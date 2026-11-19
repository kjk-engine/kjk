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
    assert_eq!(probes.0.len(), 5);
    assert_eq!(probes.failed(), 0, "module probes failed");
}

/// `surfacemat.2da`'s grass column read per material, and the made-up
/// walls, footprints and navigation meshes beside it (no installs needed;
/// proposed 2026-10-08; the funnel past a gate between two blocks' corners
/// 2026-10-28). The rooms whose drawn triangles are left out of the level
/// (`rooms_out_of_sight`, the Jedi council corridor) were added and
/// approved 2026-11-15. The placeables' drawn shapes, the furniture edge walls,
/// the low benches (issue 442) and the steep faces left out of a clip were
/// approved 2026-11-16.
#[test]
fn surface_probes() {
    probed("surfaces", &collision_odyssey::probes::probes(), 21);
}

/// `audio::probes`: a stereo signal through the code that fills the sound
/// device's stream, for made-up devices of 1 to 8 channels, at 44.1 kHz and
/// 48 kHz, in floats and in 16-, 24- and 32-bit integers (issue 402).
#[test]
fn sound_device_layouts() {
    probed("sound device", &audio::probes::device_layouts(), 45);
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
    assert_eq!(probes.len(), 11);
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

/// Which bolt hits leave a burn (`kjk_game::bolt_hits::probes`). A bolt
/// leaves a burn when it is a critical hit, a Power Blast, a Sniper Shot or
/// the killing shot, or deals at least 15% of the target's maximum
/// vitality. Any other bolt leaves a burn only when the body's last bolt
/// burn was laid at least 500 ms before. A hit on a bare head never leaves a
/// burn. A hit on an armoured head (a droid's, or a helmet such as a Sith
/// trooper's or a Mandalorian's) burns as a hit on the chest does. A body
/// with no maximum vitality counts no hit as heavy. Two hits in
/// one snapshot are gated one after the other. A pistol's burn is small, a
/// rifle's larger and a Power Blast's the largest. A hit is matched to the
/// shooter's bolt drawn in the last 250 ms whose line passes nearest the hit
/// location, if it passes within 1.5 m. A bolt already more than one tick's
/// flight past the hit location missed and is never matched. That line is
/// the bolt as drawn, leaving the gun and joining its own way. The hit's
/// light sits where the line passes the hit location. Needs `--features
/// rig`; no installs (proposed 2026-11-18, awaiting approval).
#[test]
fn bolt_hits() {
    #[cfg(not(feature = "rig"))]
    println!("bolt_hits skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("bolt hits", &kjk_game::bolt_hits::probes(), 16);
}

/// The burns bolts leave on bodies (`kjk_game::body_marks::probes`). A mark
/// keeps a triangle that crosses its depth window and drops one wholly in
/// front of it. A lightsaber's mark on a curved strip keeps no triangle
/// farther than its reach from the hit across the blade, and keeps a
/// triangle that crosses its whole depth window. A bolt burn's flash is near white when
/// laid, red at 300 ms and gone at 600 ms. Its embers glow until 3500 ms.
/// Its burn stays whole until 6000 ms and fades out by 8000 ms. A body keeps
/// at most 6 bolt burns: a seventh takes the place of the oldest bolt burn,
/// never of a lightsaber mark, and never of a burn on another body. A bolt's
/// line meets a body's front before its back. A line that meets the head
/// first leaves no burn. A bolt burn at the neck covers neither the head's
/// mesh nor a triangle skinned to the head. A bolt as drawn that passes
/// beside the body is moved to pass through its aim point, so it burns the
/// side it came from, even when the aim point lies behind the body's middle.
/// A line that meets nothing, moved or not, puts the burn on the skin
/// nearest a point 0.3 m from the chest toward the shooter that is not the
/// head's. A droid, with no hit location nodes, is aimed at the chest its
/// movement state gives, and its burn goes on its front at that height. Needs `--features rig`; no installs (proposed
/// 2026-11-18, awaiting approval).
#[test]
fn bolt_burns() {
    #[cfg(not(feature = "rig"))]
    println!("bolt_burns skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("bolt burns", &kjk_game::body_marks::probes(), 11);
}

/// A bolt burn on armour (`kjk_game::body_marks::armour_probes`). The Sith
/// trooper (`g_sithtroop01`) is loaded as the scene loads it and posed at
/// rest, and a pistol's bolt strikes its chest from the front. The burn is
/// made on at least one drawn mesh of its body model, never of its head, and
/// every triangle of the burn lies within 5 cm of the armour. Needs
/// `--features rig`; without a K1 install the check is skipped (proposed
/// 2026-11-18, awaiting approval).
#[test]
fn armour_burns() {
    #[cfg(not(feature = "rig"))]
    println!("armour_burns skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed(
        "armour burns",
        &kjk_game::body_marks::armour_probes::probes(),
        1,
    );
}

/// The firing motion (`kjk_game::recoil::probes`): each shot pushes a gun
/// back along its barrel and lifts its muzzle. At the hip the push is the
/// wield's reference push (a blaster pistol 5 cm and 3°, a blaster rifle
/// 8 cm and 1.2°) times the gun's kick rule against the reference gun's,
/// from half to twice. A gun held like a rifle is pushed back less the
/// further it is raised, down to 2.5 cm fully raised. The push is exactly
/// nothing 1.5 seconds after a lone shot, and no 8 ms step moves it by more
/// than the share of its own push that one step of its rise covers. The
/// gun-side collar bone turns back, a rifle's chest turns, and a pistol's
/// forearm and upper arm follow. Each turn is 0 before the shot and
/// exactly 0 once the wield's settle time has passed, and its size follows
/// the gun's kick, from half to twice the wield's reference gun. A pair
/// moves only the side that fired. Rapid fire, even 40 ms apart, never
/// passes a single shot's peak and keeps the same peaks for 5 seconds. No
/// 8 ms step moves a bone by more than a quarter of its peak, or by more
/// than 3°. A dead or unarmed body does not move, and the body looked
/// through in first person turns no bone. From the third shot of a streak
/// with no gap over 1100 ms, the chest leans 4° forward within 300 ms. It
/// holds the lean until 1100 ms after the last shot and straightens over
/// the next 400 ms, with no 8 ms step over 0.2°. A new streak starts with
/// no lean, and the body looked through does not lean. On a posed KOTOR
/// body, a rifle shot never takes the support hand more than 1 cm further
/// from its grip, moves the shoulder back rather than up, moves the muzzle
/// back more than up, and moves it sideways no more than half as far as
/// back. A right-hand shot of a pair leaves the left gun within 3 mm. A
/// fully raised rifle, its stock at the shoulder and the cheek resting on
/// it, comes within 3 cm of the head before a shot and stays at least 1 cm
/// from it through the shot, and the right eye stands above the rifle
/// within 4 cm of its barrel. The support hand's palm stays within 1 cm of
/// its grip on a raised rifle aimed level, 10 degrees up and 10 degrees
/// down, through a shot. On the same rifle aimed the same three ways, each
/// forearm stays no more than 1.6 cm inside the chest's mesh through a shot, and the arms stay
/// at least 1 cm from the head. A fully raised blaster rifle's stock stands within 3 cm of the
/// gun-side shoulder joint's height and in front of it, and every point of the
/// rifle stays at least 8 cm from that joint and the upper arm's bone
/// through a shot, and at the shot's peak the shoulder joint goes back at
/// least 60% as far as the stock. The lowest ring of the head's neck stays
/// joined to the body's collar, moving no more than 5 mm off it, with the
/// rifle raised and aimed the same three ways through a shot and with the
/// head turned 60 degrees to each side. Every K1 gun model of its own
/// shape, raised on Carth's body and on a player woman's (`pfbal`, head
/// `pfha01`) and fired once, stays at least 1 cm from the head; a gun at
/// the shoulder points within 0.1 degrees of its target, with its stock's
/// rear end in front of the shoulder joint, neither forearm more than 1.5 cm
/// into the chest, and the trigger elbow no higher than the shoulder. Pressing or letting go of [Aim] around a
/// rifle shot never makes the firing motion alone move the muzzle more than
/// 2 cm in a step. A rifle fired once a second breathes as a body that does
/// not fire. Needs `--features rig`; the posed checks need JA and KOTOR
/// installed (proposed 2026-11-17, changed 2026-11-18, awaiting approval).
#[test]
fn firing_motion() {
    #[cfg(not(feature = "rig"))]
    println!("firing_motion skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("firing motion", &kjk_game::recoil::probes(), 31);
}

/// The aim pose (`kjk_game::aim_pose_probes`): a body raising its gun to
/// aim follows the sim's raise with no step larger than a tenth of the
/// way, each wield's gun hand stands 15 to 30 cm below the eyes, a walking body's
/// feet keep the walk, and another character holds its gun up for 600 ms
/// after a shot. Presses and releases of [Aim] and of a charge during a
/// pistol's or a rifle's shot never make the gun hand jump, and the fade
/// that smooths a change of the guns' state hides no jump of a torso bone
/// larger than 45°. A Sniper Shot's sight shows only while it is steadied.
/// While the watched character aims, the third-person camera comes in to
/// 80% of its range, moves out to 130% of its shoulder offset and narrows
/// its field of view to 75%, over the raise's 200 ms with no frame moving it
/// more than 15% of the way. The sim's crosshair line stays on the camera
/// throughout. In first person only the field of view narrows.
/// Needs `--features rig`; the pose checks need JA installed (proposed
/// 2026-11-17).
#[test]
fn aim_pose() {
    #[cfg(not(feature = "rig"))]
    println!("aim_pose skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("aim pose", &kjk_game::aim_pose_probes::probes(), 11);
}

/// The equip page's attack and damage lines (`kjk_game::character::probes`)
/// count what combat counts: Strength or Dexterity, upgrade dice, damage
/// bonuses and damage of other types, and the fists in an empty right hand.
/// Needs `--features rig`; no installs (proposed and approved 2026-11-16).
/// The `equip_numbers_double_weapon` check was proposed on 2026-11-17. A
/// double weapon shows a line for each end, the main hand's and the off
/// hand's, with Master Two-Weapon Fighting at 0 and -2.
#[test]
fn equip_numbers() {
    #[cfg(not(feature = "rig"))]
    println!("equip numbers skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("equip numbers", &kjk_game::character::probes(), 5);
}

/// A guest's look in co-op (`kjk_game::sim_host::look_probes`): a guest
/// driving a companion whose view carries a pitch delta, and the host
/// leading through such a companion, each reach the view's pitch limit both
/// ways. Needs `--features rig`; no installs.
#[test]
fn coop_look() {
    #[cfg(not(feature = "rig"))]
    println!("coop_look skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("look", &kjk_game::sim_host::look_probes(), 2);
}

/// The world held after an install until its area can be drawn
/// (`kjk_game::sim_host::loading_hold_probes`, also `kjk sim probes --only
/// "loading hold"`). An install not yet looked at holds the world, no
/// install holds nothing, an area a renderer has not drawn holds it, and
/// so does the next install. Without a renderer the world goes on at once,
/// and an area never drawn holds it for the longest wait and no more.
/// Needs `--features rig`; no installs (proposed 2026-11-15, issue 410,
/// not yet approved).
#[test]
fn loading_hold() {
    #[cfg(not(feature = "rig"))]
    println!("loading_hold skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed(
        "loading hold",
        &kjk_game::sim_host::loading_hold_probes(),
        2,
    );
}

/// The headgear a conversation hides (`kjk_game::headgear::probes`): a DLG
/// that sets `UnequipHItem` hides each line's speaker's and listener's head
/// item until the conversation ends, as KOTOR 2's executable does; one
/// without it hides none. Needs `--features rig`; no installs (approved
/// 2026-11-16).
#[test]
fn conversation_hides_headgear() {
    #[cfg(not(feature = "rig"))]
    println!(
        "conversation_hides_headgear skipped: run with --features rig (builds the game's library)"
    );
    #[cfg(feature = "rig")]
    probed("headgear", &kjk_game::headgear::probes(), 1);
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

/// A character's look follows its alignment as both executables dress one
/// (`kjk::probes::alignment_looks`, issue 330). The dark steps start at 40,
/// 30, 20 and 10. A bare body darkens at 30 in KOTOR and at 40 in KOTOR 2,
/// and only KOTOR 2 wears a head's `alttexture`. With a K1 install, the
/// reporter's player takes the dark faces of `heads.2da` while its clothes
/// stay, its bare body takes `texaevil`, and its portrait the darkest
/// `portraits.2da` column. With a TSL install, the player's face is its
/// `alttexture` at neutral and its darkest one at 5, its bare body dark at
/// 40, Atton's face dark at 35, a Twi'lek's head its `alttexture`, and the
/// portrait the darkest. A creature placed dark wears its dark face on
/// every textured mesh of its head. Needs `--features rig`; a check whose
/// install is missing is skipped. (proposed and approved 2026-11-12)
#[test]
fn alignment_looks() {
    #[cfg(not(feature = "rig"))]
    println!("alignment_looks skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("alignment looks", &kjk::probes::alignment_looks(), 4);
}

/// A creature the scene cannot draw is named with the reason
/// (`kjk::probes::undrawn_creature`). With a K1 install, Bib Surool is drawn
/// from his own template in Javyar's Cantina (`tar_m03ae`). A look of an
/// `appearance.2da` row past the table's end is not drawn, and the reason
/// names that row. Needs `--features rig`; without a K1 install both checks
/// are skipped. (proposed and approved 2026-11-12)
#[test]
fn undrawn_creature_named() {
    #[cfg(not(feature = "rig"))]
    println!("undrawn_creature_named skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed(
        "undrawn creature named",
        &kjk::probes::undrawn_creature(),
        2,
    );
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

/// The rooms the camera is in for the room culling
/// (`kjk::probes::camera_rooms`), on rooms made in the test. Over a floor
/// the camera is in that floor's room. Against a wall, over no floor, it
/// is in every room with a floor whose bounds hold it, so a camera in the
/// wall between two rooms shows what both rooms see. A sky's bounds do not
/// count. Outside every room it is in none, and every room shows. Needs
/// `--features rig`; no installs. (proposed 2026-11-15, #464)
#[test]
fn camera_rooms() {
    #[cfg(not(feature = "rig"))]
    println!("camera_rooms skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("camera rooms", &kjk::probes::camera_rooms(), 4);
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
/// body's shadow. A lamp sees a character when the line from the lamp is
/// clear, or when the lamp hangs more than 2 m over the chest and the line
/// from 2 m over the chest under it is clear. Needs `--features rig`; no
/// installs. (proposed 2026-11-11; the junk pile and the fill added for
/// #328; approved 2026-11-12; the lamp's sight added 2026-11-12, approved
/// on 2026-11-16)
#[test]
fn capsule_shadows() {
    #[cfg(not(feature = "rig"))]
    println!("capsule_shadows skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("capsule shadows", &kjk::probes::capsule_shadows(), 9);
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
/// the same. The audit has 13 checks. A drawn tip 0.2 m from the sim's tip
/// in `stand` gives a blade-agreement of 0.2 m. The same frames in `attack`
/// give no blade-agreement value, and a growing blade gives 0. An MDA
/// blade-agreement of 0.12 m fails against a JA-rules value of 0.07 m and
/// passes against one of 0.12 m. Needs `--features rig` (kjk's library); no
/// installs.
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
    use kjk::anim_audit::checks::{self, Check, Still, Worst, measure, measure_state};
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
            (Check::BladeAgreement, 0.12, Some(0.07), 0.10, true),
            (Check::BladeAgreement, 0.12, Some(0.12), 0.125, false),
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
        // Blade-agreement is measured only in the states where a stance set
        // plays, and a growing blade is not measured.
        assert_eq!(Check::ALL.len(), 13, "the audit's checks");
        for (label, growing, expected) in [
            ("stand", false, Some(0.2)),
            ("attack", false, None),
            ("stand", true, Some(0.0)),
        ] {
            let apart = frames(4, |i| {
                let blade = Blade {
                    growing,
                    ..blade(AHEAD[0], AHEAD[1], AHEAD, 0.2)
                };
                frame(i, false, vec![blade])
            });
            let worst = measure_state(
                label,
                (None, &apart),
                &checks::holds(&apart),
                true,
                Still::Hands,
            );
            let found = worst.iter().find(|w| w.check == Check::BladeAgreement);
            let what = format!("blade-agreement in {label}, growing {growing}");
            println!("{what}: {found:?}");
            match expected {
                Some(expected) => near(&what, found.expect("blade-agreement").value, expected),
                None => assert!(found.is_none(), "{what}"),
            }
        }
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

/// A trigger or placeable made from its template takes its tag, its
/// conversation and its scripts from the template, as both games'
/// executables do. A mod's GIT entry that also writes an empty `Tag`,
/// `Conversation` or `OnEnter` changes none of them (issue 468: a mod's
/// Undercity lost Mission's trigger tag, so she never came). A trigger's own
/// `Tag` that is not empty still names it. No installs (approved 2026-11-16).
#[test]
fn git_entry_keeps_template_fields() {
    use asset_odyssey::made::{blank_module, gff, node, resref};
    use asset_odyssey::module::ModuleIr;
    use fmt_odyssey::gff::GffValue;

    let text = |value: &str| GffValue::String(value.as_bytes().to_vec());
    let entry = |template: &str, tag: &str| {
        node(vec![
            ("TemplateResRef", resref(template)),
            ("Tag", text(tag)),
            ("Conversation", resref("")),
            ("OnEnter", resref("")),
        ])
    };
    let mut files = blank_module(Vec::new(), &["Room_A"]);
    files.0.insert(
        "area.git".into(),
        gff(
            b"GIT ",
            vec![
                (
                    "TriggerList",
                    GffValue::List(vec![entry("trg", ""), entry("trg", "own_tag")]),
                ),
                ("Placeable List", GffValue::List(vec![entry("plc", "")])),
            ],
        ),
    );
    let template = |kind: &[u8; 4], tag: &str| {
        gff(
            kind,
            vec![
                ("Tag", text(tag)),
                ("Conversation", resref("talk")),
                ("OnEnter", resref("k_enter")),
            ],
        )
    };
    files
        .0
        .insert("trg.utt".into(), template(b"UTT ", "trg_tag"));
    files
        .0
        .insert("plc.utp".into(), template(b"UTP ", "plc_tag"));
    let module = ModuleIr::build("blank", &files).expect("the module builds");
    let seen: Vec<String> = module.areas[0]
        .instances
        .iter()
        .map(|instance| {
            format!(
                "{:?} {} {} {:?}",
                instance.kind,
                String::from_utf8_lossy(&instance.tag),
                instance.conversation,
                instance.scripts
            )
        })
        .collect();
    let enter = r#"[("OnEnter", "k_enter")]"#;
    assert_eq!(
        seen,
        [
            format!("Placeable plc_tag talk {enter}"),
            format!("Trigger trg_tag talk {enter}"),
            format!("Trigger own_tag talk {enter}"),
        ],
        "kind, tag, conversation and scripts of each placed object"
    );
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

/// A creature's body or head whose texture has alpha and no TXI is drawn
/// solid in an area with no env map (`kjk_game::material::worn_surface`,
/// `creatures.look.solid`; a kjk choice: under TAA the world showed
/// through Bastila's shoulders in the Taris Sith base, which names no env
/// map). Any other model keeps blending by its alpha, and so do a mesh with
/// a transparency hint and a texture whose TXI names an env map or a
/// decal. Needs `--features rig` (the game's library); no installs
/// (proposed 2026-11-13, not yet approved).
#[test]
fn creature_alpha_without_envmap_solid() {
    #[cfg(not(feature = "rig"))]
    println!("creature_alpha_without_envmap_solid skipped: run with --features rig");
    #[cfg(feature = "rig")]
    {
        use asset_odyssey::texture::TextureIr;
        use fmt_odyssey::texture::Texture;
        use fmt_odyssey::txi::Txi;
        use kjk_game::material::{Surface, surface, worn_surface};

        // A 2×2 uncompressed 32-bit TGA, alpha 8 bits, its texels' alpha low.
        let mut tga = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 32, 8];
        for _ in 0..4 {
            tga.extend_from_slice(&[40, 40, 40, 128]);
        }
        let texture = Texture::parse_tga(&tga).expect("the TGA parses");
        // (TXI line, worn by a creature, transparency hint, surface drawn)
        for (txi, worn, hint, expected) in [
            (None, true, 0, Surface::Opaque),
            (None, false, 0, Surface::OpaqueAlpha),
            (None, true, 2, Surface::OpaqueAlpha),
            (
                Some("envmaptexture CM_Baremetal"),
                true,
                0,
                Surface::OpaqueEnvMasked,
            ),
            (Some("decal 1"), true, 0, Surface::Blend),
        ] {
            let txi = txi.map(|line| Txi::parse(format!("{line}\r\n").as_bytes()));
            let facts = TextureIr::from_texture(&texture, txi.as_ref())
                .expect("the TGA decodes")
                .facts;
            let drawn = worn_surface(
                surface((hint, true), 1.0, (Some(&facts), &[&facts])),
                &[&facts],
                worn,
                hint,
            );
            assert_eq!(drawn, expected, "worn {worn}, hint {hint}, TXI {txi:?}");
        }
    }
}

/// A punch-through texture (TXI `blending punchthrough`) is cut out, not
/// blended (`kjk_game::material::Surface::Cutout`): each texel is solid or,
/// at alpha 0, gone, as KOTOR's executable draws it with no blending and its
/// alpha test `GL_GREATER 0`. The Kashyyyk Wookiees' fur is one, and
/// blended, its faint edges hid parts of their bodies. A skinned mesh is cut
/// out too, and a TXI that also names an env map keeps the env map's mask.
/// Needs `--features rig` (the game's library); no installs (proposed
/// and approved 2026-11-12).
#[test]
fn punch_through_cut_out() {
    #[cfg(not(feature = "rig"))]
    println!("punch_through_cut_out skipped: run with --features rig");
    #[cfg(feature = "rig")]
    {
        use asset_odyssey::texture::TextureIr;
        use fmt_odyssey::texture::Texture;
        use fmt_odyssey::txi::Txi;
        use kjk_game::material::{Surface, surface};

        // A 2×2 uncompressed 32-bit TGA, alpha 8 bits, its texels' alpha low.
        let mut tga = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 32, 8];
        for _ in 0..4 {
            tga.extend_from_slice(&[40, 40, 40, 128]);
        }
        let texture = Texture::parse_tga(&tga).expect("the TGA parses");
        for (txi, skinned, expected) in [
            ("blending punchthrough", true, Surface::Cutout),
            ("blending punchthrough", false, Surface::Cutout),
            (
                "blending punchthrough\r\nenvmaptexture CM_Baremetal",
                true,
                Surface::OpaqueEnvMasked,
            ),
            ("blending additive", false, Surface::Additive),
        ] {
            let txi = Txi::parse(format!("{txi}\r\n").as_bytes());
            let facts = TextureIr::from_texture(&texture, Some(&txi))
                .expect("the TGA decodes")
                .facts;
            let drawn = surface((0, skinned), 1.0, (Some(&facts), &[&facts]));
            assert_eq!(drawn, expected, "TXI {txi:?}, skinned {skinned}");
        }
    }
}

/// A room's mesh whose diffuse is `NULL` and that has no lightmap is drawn
/// untextured (`kjk_game::material::mesh_drawn`), as KOTOR 2's executable
/// draws it. The Ebon Hawk's rooms lie over such a plane. Without it, the
/// hyperspace sky showed blue through the seams of the floor (issue 441).
/// Such a mesh that casts shadows is not drawn: the Dantooine enclave's
/// rooms each have one, a box whose lid hid the sky above the council
/// chamber's round opening. Such a mesh on a creature, a placeable or a door is still not drawn. A
/// mesh that names a texture of which none loaded is not drawn either. An
/// untextured mesh whose alpha is below 1 is blended, not drawn solid.
/// Needs `--features rig` (the game's library); no installs (proposed
/// 2026-11-16, not yet approved).
#[test]
fn untextured_mesh_drawn() {
    #[cfg(not(feature = "rig"))]
    println!("untextured_mesh_drawn skipped: run with --features rig");
    #[cfg(feature = "rig")]
    {
        use kjk_game::material::{Surface, mesh_drawn, surface};

        // ((room, casts shadows), (diffuse named, loaded), (lightmap named,
        // loaded), drawn)
        for (room, texture, lightmap, drawn) in [
            ((true, false), (false, false), (false, false), true),
            ((true, true), (false, false), (false, false), false),
            ((false, false), (false, false), (false, false), false),
            ((true, false), (true, false), (false, false), false),
            ((true, false), (true, false), (true, false), false),
            ((true, false), (true, false), (true, true), true),
            ((true, true), (true, true), (false, false), true),
            ((false, false), (false, false), (true, true), true),
            ((false, false), (true, true), (false, false), true),
        ] {
            assert_eq!(
                mesh_drawn(room, texture, lightmap),
                drawn,
                "room and shadow {room:?}, diffuse {texture:?}, lightmap {lightmap:?}"
            );
        }
        for (alpha, expected) in [
            (1.0, Surface::Opaque),
            (0.0, Surface::Blend),
            (0.25, Surface::Blend),
        ] {
            assert_eq!(
                surface((0, false), alpha, (None, &[])),
                expected,
                "alpha {alpha}"
            );
        }
    }
}

/// Every TGA layout a mod's texture may come in reads, with its rows bottom
/// to top as a TPC's (`fmt_odyssey::texture::Texture::parse_tga_layers`):
/// true colour at 32, 24 and 16 bits, raw or run-length encoded, grey with
/// and without alpha, colour-mapped, an unused colour map skipped; a TXI's
/// `cube` stacks six faces in it, and a TXI flip-book's sheet is cut into
/// its frames (`parse_tga_frames`). A texture that did not read was drawn
/// white (issue 276). The descriptor's corner bits change nothing, because
/// TSL's executable never reads them: a file said to start at the top left,
/// or to run right to left, reads its texels in stored order, bottom row
/// first (issue 559). No installs (proposed 2026-11-10, the flip-book sheet
/// 2026-11-12; approved 2026-11-12; its frames' order in the stored bands
/// changed and approved the same day; the corner bits ignored proposed
/// 2026-11-17, not yet approved).
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
    // The image's texels in a file's order: bottom row first, each row left
    // to right, whatever corner the descriptor names. TSL's executable never
    // reads the descriptor's corner bits, so a file draws its stored texels
    // that way in the game (issue 559).
    let texels = || -> Vec<[u8; 4]> {
        [1, 0]
            .iter()
            .flat_map(|row| IMAGE[row * 2..row * 2 + 2].to_vec())
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
        tga.extend(texels().into_iter().flat_map(texel));
        tga
    };
    // Run-length encoded: one raw packet of all four texels.
    let rle = |depth: u8, texel: &dyn Fn([u8; 4]) -> Vec<u8>| {
        let mut tga = header(10, depth, 8, &[], 0);
        tga.push(3);
        tga.extend(texels().into_iter().flat_map(texel));
        tga
    };
    // Colour-mapped: a palette of the four colours, indexed in order.
    let indexed = |kind: u8| {
        let palette: Vec<u8> = IMAGE.into_iter().flat_map(bgra).collect();
        let mut tga = header(kind, 8, 8, &palette, 32);
        let index = |texel: [u8; 4]| IMAGE.iter().position(|c| *c == texel).unwrap() as u8;
        tga.extend(texels().into_iter().map(index));
        tga
    };
    let mut unused_map = header(2, 24, 0, &[1, 2, 3, 4, 5, 6], 24);
    unused_map.extend(texels().into_iter().flat_map(bgr));
    let cases: [(&str, Vec<u8>, Reads); 11] = [
        ("32-bit", file(2, 32, 8, &bgra), |c| c),
        ("32-bit, top-down bit", file(2, 32, 0x28, &bgra), |c| c),
        ("32-bit, right-left bit", file(2, 32, 0x18, &bgra), |c| c),
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
    // A flip-book sheet: a 4×2 image of 2×2 frames, each 2×1, each texel
    // its stored place. The frames come out in the order the file stores
    // them, as TSL's executable cuts them, left to right from the band
    // stored first, each its own layer (a mod's glowing blade edge drew its
    // whole sheet across the blade; a console's frames played out of
    // order). So it is whether the file stores its bottom row first or its
    // top row first.
    for descriptor in [0, 0x20] {
        let mut sheet = header(2, 24, descriptor, &[], 0);
        sheet[12] = 4;
        sheet.extend((0..8u8).flat_map(|texel| [texel, texel, texel]));
        let texture = Texture::parse_tga_frames(&sheet, 2, 2).expect("the sheet parses");
        let frames: Vec<Vec<u8>> = (texture.layers.iter())
            .map(|frame| frame[0].data.iter().step_by(3).copied().collect())
            .collect();
        assert_eq!(frames, [[0, 1], [2, 3], [4, 5], [6, 7]]);
        assert!(Texture::parse_tga_frames(&sheet, 3, 2).is_err());
    }
}

/// A TGA's mip levels are made when it is loaded
/// (`asset_odyssey::texture::TextureIr::from_texture`). A TGA stores its
/// first level alone, and a mod's textures, almost all TGAs, shimmered at a
/// distance with none below it (a Discord report of an HD texture pack's
/// Taris skyline, and issue 407's grate). The chain runs down to one texel,
/// each level half the one above it, whatever the image's size. Its first
/// level is the image as it was. Each texel below it averages the stored
/// values of the four it covers, alpha too, as the games' own TPCs were
/// made (their stored second levels match that, not an average of light or
/// one whose colours count by alpha), so one opaque red texel and three
/// clear black ones make a dark red texel a quarter opaque, with or without
/// an env map. A TXI with `mipmap 0` (which the
/// games give their screens' art) keeps the one level, and a TPC's stored
/// levels stay as they are, one or many. No installs (proposed 2026-11-15,
/// not yet approved).
#[test]
fn tga_mips_made() {
    use asset_odyssey::texture::TextureIr;
    use fmt_odyssey::texture::Texture;
    use fmt_odyssey::txi::Txi;

    // A 32-bit TGA of `width` × `height`, its texels BGRA bottom to top.
    let tga = |width: u8, height: u8, texels: &[[u8; 4]]| {
        let mut tga = vec![
            0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, width, 0, height, 0, 32, 8,
        ];
        tga.extend(texels.iter().flat_map(|[r, g, b, a]| [*b, *g, *r, *a]));
        tga
    };
    let loaded = |bytes: &[u8], txi: Option<&str>| {
        let texture = Texture::parse_tga(bytes).expect("the TGA parses");
        let txi = txi.map(|text| Txi::parse(text.as_bytes()));
        let ir = TextureIr::from_texture(&texture, txi.as_ref()).expect("it decodes");
        (texture, ir)
    };
    let grey = |count: usize| -> Vec<[u8; 4]> {
        (0..count).map(|texel| [texel as u8, 128, 7, 255]).collect()
    };
    for (width, height, sizes) in [
        (8u8, 8u8, vec![(8, 8), (4, 4), (2, 2), (1, 1)]),
        (6, 3, vec![(6, 3), (3, 1), (1, 1)]),
    ] {
        let (texture, ir) = loaded(
            &tga(
                width,
                height,
                &grey(usize::from(width) * usize::from(height)),
            ),
            None,
        );
        let lengths: Vec<usize> = ir.mips.iter().map(Vec::len).collect();
        let expected: Vec<usize> = sizes.iter().map(|(w, h)| w * h * 4).collect();
        assert_eq!(lengths, expected, "{width}x{height}");
        assert_eq!(ir.mips[0], texture.stored_rgba(0, 0).expect("it decodes"));
    }

    let edge = tga(
        2,
        2,
        &[[255, 0, 0, 255], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
    );
    let (_, ir) = loaded(&edge, None);
    assert_eq!(ir.mips[1], [64, 0, 0, 64], "dark red, a quarter opaque");
    // Four texels of one colour average to that colour, every value of it.
    for value in 0..=255u8 {
        let (_, ir) = loaded(&tga(2, 2, &[[value, 0, 255, 255]; 4]), None);
        assert_eq!(ir.mips[1], [value, 0, 255, 255], "value {value}");
    }
    let (_, ir) = loaded(
        &edge,
        Some(
            "envmaptexture cm_baremetal
",
        ),
    );
    assert_eq!(
        ir.mips[1],
        [64, 0, 0, 64],
        "an env map's mask averages alike"
    );

    let (_, ir) = loaded(
        &tga(8, 8, &grey(64)),
        Some(
            "mipmap 0
",
        ),
    );
    assert_eq!(ir.mips.len(), 1, "mipmap 0 keeps one level");

    // An uncompressed RGBA TPC of 4×4: its header, then its stored levels.
    let tpc = |levels: &[Vec<u8>]| {
        let mut tpc = vec![0u8; 128];
        tpc[8] = 4;
        tpc[10] = 4;
        tpc[12] = 4;
        tpc[13] = levels.len() as u8;
        tpc.extend(levels.concat());
        tpc
    };
    let level = |side: usize, value: u8| [value, 9, 200, 255].repeat(side * side);
    for levels in [
        vec![level(4, 10)],
        vec![level(4, 10), level(2, 99), level(1, 3)],
    ] {
        let texture = Texture::parse_tpc(&tpc(&levels)).expect("the TPC parses");
        let ir = TextureIr::from_texture(&texture, None).expect("it decodes");
        assert_eq!(ir.mips, levels, "a TPC's {} stored levels", levels.len());
    }
}

/// A texture's TXI is picked as KOTOR 2's executable picks it
/// (`CAuroraInterface::GetResource`, `fmt_odyssey::texture::texture_txi`).
/// A loose TXI of the texture's name comes first, even an empty one.
/// Without one, a TPC's own stored TXI is used. A TGA has none of its own,
/// so it takes the TXI of the TPC of its name beneath it. High Quality
/// Blasters ships its gun TGAs without TXIs over the game's TPCs, whose TXI
/// names an env map; kjk read none and drew the guns see-through (issue
/// 171). A TGA with no TPC beneath it and no loose TXI has none. A TGA that
/// keeps its stock TPC's env map is env-masked, so its mip levels count
/// every texel alike. No installs (proposed 2026-11-15, not yet approved).
#[test]
fn texture_txi_order() {
    use asset_odyssey::texture::TextureIr;
    use fmt_odyssey::ResType;
    use fmt_odyssey::texture::{Texture, texture_txi};
    use fmt_odyssey::txi::Txi;

    // A 2×2 uncompressed RGBA TPC with `txi` stored after its pixels.
    let tpc = |txi: &str| {
        let mut tpc = vec![0u8; 128];
        tpc[8] = 2;
        tpc[10] = 2;
        tpc[12] = 4;
        tpc[13] = 1;
        tpc.extend([200u8, 200, 200, 255].repeat(4));
        tpc.extend(txi.as_bytes());
        tpc
    };
    // A 2×2 32-bit TGA: one opaque red texel, three clear black ones.
    let mut tga = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 32, 8];
    for texel in [[0, 0, 255, 255], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]] {
        tga.extend(texel);
    }
    let stock = tpc("envmaptexture CM_Baremetal
");
    let own = Texture::parse_tpc(&tpc("blending additive
"))
    .expect("the TPC parses");
    let mod_tga = Texture::parse_tga(&tga).expect("the TGA parses");
    let parsed = |text: &str| Some(Txi::parse(text.as_bytes()));
    // What the resolver holds for the name: a loose TXI, a TPC.
    let holding = |txi: Option<&'static str>, tpc: Option<&Vec<u8>>| {
        let tpc = tpc.cloned();
        move |restype: ResType| match restype {
            ResType::TXI => txi.map(|text| text.as_bytes().to_vec()),
            ResType::TPC => tpc.clone(),
            _ => None,
        }
    };
    for (what, texture, read, expected) in [
        (
            "a loose TXI over a TPC's own",
            &own,
            holding(
                Some(
                    "decal 1
",
                ),
                Some(&stock),
            ),
            parsed("decal 1"),
        ),
        (
            "an empty loose TXI over a TPC's own",
            &own,
            holding(Some(""), Some(&stock)),
            parsed(""),
        ),
        (
            "a TPC's own",
            &own,
            holding(None, Some(&stock)),
            parsed("blending additive"),
        ),
        (
            "a TGA over a stock TPC",
            &mod_tga,
            holding(None, Some(&stock)),
            parsed("envmaptexture CM_Baremetal"),
        ),
        (
            "a loose TXI over a TGA's stock TPC",
            &mod_tga,
            holding(
                Some(
                    "decal 1
",
                ),
                Some(&stock),
            ),
            parsed("decal 1"),
        ),
        (
            "a TGA with no TPC beneath",
            &mod_tga,
            holding(None, None),
            None,
        ),
    ] {
        assert_eq!(texture_txi(Some(texture), read), expected, "{what}");
    }

    let inherited = texture_txi(Some(&mod_tga), holding(None, Some(&stock)));
    let ir = TextureIr::from_texture(&mod_tga, inherited.as_ref()).expect("it decodes");
    assert_eq!(ir.facts.envmap.as_deref(), Some("cm_baremetal"));
    // Every texel counted alike, by its stored value: a quarter of full red
    // is 64, and so is a quarter of full alpha.
    assert_eq!(ir.mips[1], [64, 0, 0, 64]);
}

/// A texture larger than the GPU takes is reduced to fit
/// (`asset_odyssey::texture::TextureIr::within`), as the scene and the
/// screens upload it. The Swoop Monitor Replacement Pack's 22528-texel-wide
/// sheet in the Nar Shaddaa docks ended the game on a GPU that takes 16384
/// a side. Its largest levels are dropped while a smaller one follows. One
/// level left too large is decoded and halved, each texel the average of
/// the two by two it covers, as are each frame of a flip-book and each
/// face of a cube. A texture that fits is kept as it is. No installs
/// (proposed and approved 2026-11-12).
#[test]
fn oversized_textures_fit() {
    use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};

    let texture = |width: u32, height: u32, encoding: Encoding, mips: Vec<Vec<u8>>| TextureIr {
        width,
        height,
        encoding,
        mips,
        cube: false,
        frames: 1,
        facts: TextureFacts::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    };
    // An 8×2 RGBA8 image of one level: four 2×2 squares, each texel of a
    // square one of its four values.
    let texel = |x: u8, y: u8| [x * 10 + y, 0, 0, 255];
    let wide: Vec<u8> = (0..2)
        .flat_map(|y| (0..8).flat_map(move |x| texel(x, y)))
        .collect();
    let one_level = texture(8, 2, Encoding::Rgba8, vec![wide.clone()]);
    assert!(one_level.within(8).is_none(), "it fits as it is");
    let fitted = one_level.within(4).expect("it is reduced");
    assert_eq!((fitted.width, fitted.height), (4, 1));
    let reds: Vec<u8> = fitted.mips[0].iter().step_by(4).copied().collect();
    assert_eq!(
        reds,
        [5, 25, 45, 65],
        "each texel the average of its square"
    );

    // DXT1 of 8×8 with its chain (8×8, 4×4, 2×2, 1×1): the top level goes.
    let blocks =
        |width: u32, height: u32| vec![0u8; (width.div_ceil(4) * height.div_ceil(4) * 8) as usize];
    let chain: Vec<Vec<u8>> = [8, 4, 2, 1].map(|side| blocks(side, side)).to_vec();
    let fitted = (texture(8, 8, Encoding::Bc1, chain.clone()))
        .within(4)
        .expect("it is reduced");
    assert_eq!(
        (fitted.width, fitted.height, fitted.encoding),
        (4, 4, Encoding::Bc1)
    );
    assert_eq!(fitted.mips, chain[1..]);
    // One level of DXT1 too large: decoded to RGBA8, then halved.
    let fitted = (texture(8, 8, Encoding::Bc1, vec![blocks(8, 8)]))
        .within(4)
        .expect("it is reduced");
    assert_eq!(
        (fitted.width, fitted.height, fitted.encoding),
        (4, 4, Encoding::Rgba8)
    );
    assert_eq!(fitted.mips[0].len(), 4 * 4 * 4);

    // A flip-book of two frames, each reduced as its first is; a cube's
    // six faces the same.
    let mut book = texture(8, 2, Encoding::Rgba8, vec![wide.clone()]);
    book.frame_mips = vec![wide.clone(), wide.clone()]
        .into_iter()
        .map(|frame| vec![frame])
        .collect();
    book.frames = 2;
    let mut cube = texture(8, 2, Encoding::Rgba8, vec![wide.clone()]);
    cube.faces = vec![vec![wide.clone()]; 6];
    for reduced in [book.within(4), cube.within(4)] {
        let reduced = reduced.expect("it is reduced");
        let layers = reduced.frame_mips.iter().chain(&reduced.faces);
        for layer in layers {
            assert_eq!(layer, &reduced.mips, "every layer reduced alike");
        }
    }
}

/// A `baseitems.2da` row is a weapon by its columns, as the executables
/// read it (`sim::combat::Delivery::of_columns`). Read so, every weapon row
/// K1 and TSL share delivers as its number says (`of_base_item`), and a row
/// the number table lacks still fights: TSL's force pike, and the mining
/// lasers K2CP moves to rows of their own, which kjk drew as fists. The
/// columns are the games' own cells. No installs (proposed and approved
/// 2026-11-12).
#[test]
fn weapon_rows_by_columns() {
    use sim::combat::Delivery;
    use sim::combat::sheet::{Blaster, SaberShape, WeaponColumns};
    // Each row: weaponwield, weapontype, damageflags, weaponsize,
    // rangedweapon, ammunitiontype (0 for an empty cell).
    let columns = |[wield, kind, damage, size, ranged, ammunition]: [i64; 6]| {
        let cell = |value: i64| (value != 0).then_some(value);
        WeaponColumns {
            wield: cell(wield),
            kind: cell(kind),
            damage: cell(damage),
            size: cell(size),
            ranged: ranged != 0,
            ammunition: cell(ammunition),
        }
    };
    let shared: [(u16, [i64; 6]); 30] = [
        (0, [3, 1, 1, 4, 0, 0]),
        (1, [1, 1, 1, 1, 0, 0]),
        (2, [2, 1, 4, 3, 0, 0]),
        (3, [2, 1, 4, 3, 0, 0]),
        (4, [2, 1, 2, 2, 0, 0]),
        (5, [2, 1, 2, 2, 0, 0]),
        (6, [3, 1, 4, 4, 0, 0]),
        (7, [3, 1, 4, 4, 0, 0]),
        (8, [2, 1, 4096, 3, 0, 0]),
        (9, [3, 1, 4096, 4, 0, 0]),
        (10, [2, 1, 4096, 2, 0, 0]),
        (11, [0, 0, 0, 0, 0, 0]),
        (12, [4, 4, 4096, 2, 1, 1]),
        (13, [4, 4, 4096, 2, 1, 1]),
        (14, [4, 4, 4096, 2, 1, 1]),
        (15, [4, 4, 2048, 2, 1, 2]),
        (16, [4, 4, 16, 2, 1, 3]),
        (17, [4, 4, 1024, 2, 1, 4]),
        (18, [5, 4, 2048, 4, 1, 2]),
        (19, [5, 4, 4096, 4, 1, 5]),
        (20, [5, 4, 4096, 4, 1, 1]),
        (21, [5, 4, 16, 4, 1, 3]),
        (22, [5, 4, 1024, 4, 1, 4]),
        (23, [6, 4, 4096, 4, 1, 1]),
        (24, [6, 4, 4096, 4, 1, 3]),
        (77, [5, 4, 4096, 4, 1, 1]),
        (78, [3, 1, 2, 4, 0, 0]),
        (79, [3, 1, 4, 4, 0, 0]),
        (80, [2, 1, 4, 3, 0, 0]),
        (81, [0, 3, 4, 0, 0, 0]),
    ];
    for (row, cells) in shared {
        assert_eq!(
            Delivery::of_columns(&columns(cells)),
            Delivery::of_base_item(row),
            "baseitems.2da row {row}"
        );
    }
    // TSL's force pike (row 93) and K2CP's mining laser and advanced mining
    // laser (rows it appends, 104 and 105 on an unmodded TSL).
    let unshared: [(u16, [i64; 6], Delivery); 3] = [
        (93, [3, 1, 1, 4, 0, 0], Delivery::Melee),
        (
            104,
            [4, 4, 4096, 2, 1, 1],
            Delivery::Blaster(Blaster::Pistol),
        ),
        (
            105,
            [5, 4, 4096, 4, 1, 1],
            Delivery::Blaster(Blaster::Rifle),
        ),
    ];
    for (row, cells, delivery) in unshared {
        assert_eq!(Delivery::of_base_item(row), None);
        assert_eq!(Delivery::of_row(row, &columns(cells)), Some(delivery));
    }
    // A row's number wins where the games share it.
    let staff = columns([3, 1, 4096, 4, 0, 0]);
    assert_eq!(
        Delivery::of_row(8, &staff),
        Some(Delivery::Saber(SaberShape::Single))
    );
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
        mips_stored: true,
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
/// table in `Override/` over the global RIM's (proposed and approved 2026-11-12), a
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

/// A GIT creature whose template the module lacks, and which has no
/// appearance of its own, is not placed, as the original game places none.
/// One with an appearance of its own is still placed. No installs (issue
/// 478, the unseen iriaz of Restored Content for K1CP).
#[test]
fn creatures_without_a_template() {
    probed(
        "creatures without a template",
        &asset_odyssey::module::placement_probes(),
        2,
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
/// make the same levels, and a named line changed moves the digest. A guest
/// whose library is not the host's is told whether its voice files, its
/// `dialog.tlk` or its scripts differ (`named_difference`, added and
/// approved 2026-11-15). A line of only spaces, a line with no text and an empty line
/// read alike and make the same digest, so Aspyr's Mac and Windows K1
/// tables make the same levels (`blank_lines`, proposed 2026-11-15,
/// approved on 2026-11-16). No installs (approved 2026-11-05).
#[test]
fn talk_lines_digest() {
    passed("talk lines", &sim::script::dialogue_probes::talk(), 3);
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
    passed("reconcile", &sim::reconcile_probes::checks(), 15);
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
/// sweep`). The feats step's `feat_after_pick` check was added and approved
/// on 2026-11-12. It opens a feat once another pick of the same level that
/// it needs is made. The level-up checks were changed and approved the same
/// day (issue 339). They build on the character's own scores, not on the
/// scores its items add. The `skill_bonuses` check was added and approved
/// on 2026-11-12. The skills page shows a skill's own
/// ranks, its bonus and its total, and the character page colours a score
/// that items raise. The `hud_action_bar` check was added and approved on
/// 2026-11-15. The HUD draws the action bar in KOTOR 2's action slots,
/// four slots on a controller and six on the keyboard, each with its key.
/// A slot the character cannot use has its icon dimmed, an empty slot shows
/// its frame and key only, and a key left unbound draws nothing. The
/// `power_wheel_drawn` check was added on 2026-11-15. The power wheel draws
/// its highlighted entry 1.2 times as large as the others and an arc of
/// items beyond the ring. A wheel with nothing to offer draws only its
/// centre label and its key. The action bar setup checks were added on
/// 2026-11-15 and changed the same day, when the setup page became a grid
/// of icons. They are `put_by_key`, `clear_slot`, `hide_power`,
/// `pad_two_step`, `drag_icon_to_slot`, `drag_slot_to_slot`,
/// `bar_setup_layout`, `info_box_follows_pointer`,
/// `member_change_forgets_choice`, `others_bar_untouched`,
/// `abilities_opens_bar_setup` and `back_to_abilities`. A power, feat attack
/// or item dragged onto a slot goes there. A slot dragged onto another slot
/// moves or swaps, and a slot dragged off the slots is cleared. A slot's key
/// puts the icon under the pointer in that slot. A right click, H or Y
/// hides a power from the wheel or shows it again. The info box names what
/// is under the pointer and gives its description, and a hidden power's
/// description starts with a sentence that says so. Only a hidden power's
/// icon is grey on the setup page. Nothing is sent for a member that
/// another player dresses, and showing another member forgets a chosen icon
/// and a drag. The checks `hidden_icon_grey` and `power_wheel_dimmed_grey`
/// were added the same day. A disabled icon is drawn faded and grey on the
/// HUD's bar and on the wheel. On the setup page, only a hidden power's icon
/// is grey. Dimmed list rows on other screens are only faded, as before.
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
/// both games but not in an unescapable area (a script's or the ARE's),
/// nor in K1 while the party is apart or an enemy sees it
/// (`party_changed_in_play`: Luxa's arms deal on Telos, both issue 317,
/// approved 2026-11-12; its K1 cases and ARE cases, from the K1 report
/// "Party Selection Screen Issues", approved on 2026-11-16), and the
/// area's iteration passing over the dead (`area_iteration_skips_the_dead`:
/// the battle of Khoonda's end), and a companion spawned again while its
/// creature is here being that creature (`spawned_again_is_the_same`:
/// Atton's warning on Nar Shaddaa), and `k_align_movie` run as the status
/// summary closes (`summary_closed_runs_align_movie`: Visas Marr's
/// arrival), and each game's local boolean slots (K1's 96, TSL's 160:
/// `local_booleans_k1`, `local_booleans_tsl`), and the player's own
/// character kept out of `ChangeToStandardFaction` in TSL
/// (`pc_faction_tsl`: Dxun's battle circle), and a creature a script makes
/// and turns hostile at once standing up hostile
/// (`made_creature_turned_hostile`: the Exchange's Gamorreans on Telos;
/// added and approved 2026-11-14), its joining the party afterwards in the
/// same script taking it into the party's faction
/// (`made_hostile_then_joined`), and a sheet a script set on it in the step
/// being the one it stands up with (`made_sheet_set_in_the_step`; these two
/// added and approved 2026-11-14), and a companion already available not
/// made available again, its creature kept by the party table
/// (`available_again_refused`: TSLRCM's Iziz arrival and Mandalore, issue
/// 396; added and approved 2026-11-14). The sim alone, on the
/// proving ground.
#[test]
fn tsl_party_rules() {
    probed("tsl", &sim::tsl_probes::probes(), 35);
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
/// no remains; one whose bag's row is a corpse (the rancor's) is never
/// taken away, its body lying under its remains through a save and a load
/// (added and approved 2026-11-16, issue 459); the player running through where one lies does not shove its
/// body (a corpse passes through bodies, JA's dead mask); two runs end the
/// same. The sim alone.
#[test]
fn corpse_remains() {
    probed("remains", &sim::remains::probes(), 10);
}

/// Items a module gives a resref of its own, and what a creature joining
/// brings (`sim::own_item_probes`), approved 2026-11-08: two modules'
/// `g_i_datapad002` (Calo Nord's journal, the Sith base's log) are two
/// items, each read as its own module's, never one stack; two of one
/// module's still stack; a companion joining puts what it carries in the
/// party's inventory (Mission's vibroblade) and keeps what it wears. The
/// script state alone. A module's own template replaces the shared one of
/// its resref, so a machine with the import step's templates and one
/// without build the same catalog (tar_m02ae's `ptar_sbpasscrd`, added and
/// approved 2026-11-15).
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

/// The crosshair past placeables (added and approved 2026-11-14, the
/// Ithorian in the Exchange's force cage on Telos): looking straight and
/// with soft aim, a prisoner inside a placeable cage of four walls is
/// picked, and so is a bystander behind a single placeable wall, as KOTOR's
/// line of sight for selection looks on past placeables; a creature behind
/// a wall of the level itself is not. A terminal set into a level wall, its
/// middle inside the wall, is picked and seen by a use. The sim alone.
#[test]
fn crosshair_into_cages() {
    probed("crosshair cage", &sim::target::cage_probes(), 5);
}

/// Bashing (`docs/COMBAT.md`): on the proving ground
/// the player's blow on the locked gate takes off what it is worth less the
/// gate's hardness and raises its `OnDamaged` with the player as damager;
/// a creature outside the party hurts nothing; nor does a weapon on what is
/// not locked, is plot, or out-hards it, though the one it out-hards hears
/// it was attacked (`OnMeleeAttacked`, not `OnDamaged`); a script's damage
/// hurts what is not plot; Min1HP holds at 1; the gate's hit points gone,
/// it breaks open and unlocked, and neither closes nor takes more. The
/// gate broken open runs its `OnOpen` with the player as its opener, and
/// not its `OnDeath`, as both games do for a door that damage breaks
/// (added and approved 2026-11-15, the Peragus dormitory wall); a grenade blast's hit breaks a locked safe; a creature whose
/// heartbeat asks `DoDoorAction(…, DOOR_ACTION_BASH)` breaks a locked box
/// beside it; the player holding attack at the locked gate breaks it with
/// its blaster, empty-handed with its fists, and with a stun baton (the
/// sim's own shots and punches); a baton with Door Cutting (TSL's plasma
/// torch) breaks a gate of hardness 100 a plain baton leaves whole; the
/// state round-trips; two runs end the same. A plot placeable of a hostile
/// appearance (K1's Star Forge stasis tanks) that the player aims at while
/// it is hostile raises its `OnMeleeAttacked` and takes no damage; one that
/// is not plot is hurt; one a script turned neutral is spared; a blow on a
/// second tank while the player aims elsewhere is no attack (added and
/// approved 2026-11-13, issue 366). The sim alone.
#[test]
fn bash_doors() {
    probed("bash", &sim::objects::probes(), 24);
}

/// Placeables' looks and the visuals on objects (proposed 2026-10-06, the
/// Star Forge's captive Jedi): on the proving ground a captive placed on
/// is used, and its `OnUsed` leaves it off (`PlayAnimation`) and puts a
/// drain beam on it from a placeable beside it, with the drain's flash; the
/// snapshot carries both and round-trips; the flash goes after a second,
/// the beam after its time; two runs end the same. A map that a script
/// activates and then loops (issue 516, the Star Maps) holds its activation
/// for its `off2on` clip's length before the loop starts. The sim alone.
#[test]
fn placeable_looks() {
    probed("placeable looks", &sim::visuals::probes(), 8);
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
/// and disarms it. The sim alone. [approved on 2026-11-16's approval]
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

/// The player's blows on a creature that is not its foe (issue #341): on
/// the proving ground the player, bare-handed, holds attack at a neutral
/// creature for 12 s. Its punches meet the creature, which loses no hit
/// points, is never in combat and never runs its end-of-round script. A
/// foe struck the same way is in combat and its end-of-round script runs.
/// The sim alone. (proposed and approved 2026-11-12)
#[test]
fn blows_on_a_bystander() {
    probed("bystander", &sim::strikes::bystander_probes(), 3);
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
/// Power Blast fires both guns at once for one cost (owner's ask,
/// 2026-11-02), a steadied Sniper Shot fires both guns on the same step for
/// one cost (changed 2026-11-17, awaiting approval), and a gunner without
/// the feats only fires plain shots with [Feat attack]; a hostile creature with a pair fires
/// both, oftener than with one; the pair's state comes back whole from a
/// snapshot mid-burst; two runs end the same. The sim alone.
/// (approved 2026-10-25)
#[test]
fn dual_pistols() {
    probed("dual pistols", &sim::pair_probes::probes(), 12);
}

/// Hold to aim (`sim::aim_probes`). A player's shots go where the middle of
/// the screen looks, whatever the camera. Holding [Aim] raises the gun
/// fully after `weapons.aim.in_ms`, and letting go lowers it fully after
/// `weapons.aim.out_ms`. The aim and a Sniper Shot's steadying come back
/// whole from a snapshot. A snapshot from before hold to aim reads the gun
/// lowered. Putting the gun away lowers it. The scripted words `alt` and
/// `feat`, and `Command::other_mode`, hold the aim and feat attack buttons.
/// On the proving ground, a repeating blaster fully raised spreads its
/// shots by the aiming stance of `weapons.bloom.stance`, standing and
/// walking, and one still rising uses the normal spread. A character aiming covers
/// the distance a held [Walk] covers, also under Force Speed, and never
/// jumps or rolls. All seven blasters aim. A lightsaber, a vibroblade and
/// bare hands do not. A creature's held aim does nothing. A weapon switch,
/// the holster, a stun, a knockdown and a medpac each lower the gun on
/// the next step. A predicted step raises the gun as the authority's does.
/// With a blaster, [Feat attack] fires the feat attack, [Attack] counts as
/// not held while it is, and [Alt attack] does nothing. A Sniper Shot held
/// for `feats.specials.sniper_shot.steady_ms` fires one shot on the
/// release, with no spread, for one cost. Let go sooner, it fires nothing
/// and costs nothing. It may steady while the gun cools down after a shot.
/// Let go once steady, it leaves on the first step the gun is ready, so a
/// rifle's chained Sniper Shots leave one refire apart. While it steadies,
/// the character walks and cannot jump. A weapon switch, the holster, a
/// stun, a knockdown and a medpac each end it, also while a steady shot
/// waits for the gun, and nothing fires or is paid. A steady shot that
/// waits stays steady when the gun leaves the aimed state. [Next feat
/// attack] ends a steadying, a waiting shot or a Power Blast's charge, and
/// nothing fires from it or is paid twice. Without the stamina,
/// [Feat attack] fires as [Attack] does. A creature's mind fires Sniper
/// Shots without aiming. That creature carries no items (changed
/// 2026-11-19, awaiting approval). A feat attack chosen from the action bar is
/// the one [Feat attack] fires. [Feat attack] counts as an attack
/// everywhere an attack matters: a companion holding it fires nothing while
/// a conversation holds the world, and it ends stealth mode, breaks off a
/// revive and a steered walk, is an attack to the scripts, and cannot raise
/// a holstered gun while a grenade is readied.
/// The sim alone. (proposed 2026-11-17, awaiting approval)
#[test]
fn aim() {
    probed("aim", &sim::aim_probes::probes(), 40);
}

/// Every pistol is held in one hand, by its `baseitems.2da` row's
/// `weaponwield` (#338). The blaster pistol, the heavy and hold-out
/// blasters, and the ion, disruptor and sonic pistols take JA's pistol
/// ready pose and shot. The rifles, the bowcaster and the repeaters take
/// the rifle's. An ion blaster beside a blaster pistol pairs as two pistols.
/// The cells are the games' own (K1 and TSL share them). The sim alone.
/// (proposed and approved 2026-11-12)
#[test]
fn pistols_in_one_hand() {
    use sim::combat::Delivery;
    use sim::combat::saber::{Held, Kit};
    use sim::combat::sheet::{Sheet, Weapon, build};
    use sim::motion::{Anim, gun_ready, gun_shot};
    // Each base item row and its `weaponwield` and `weaponsize`.
    let gun = |(row, wield, size): (u16, i64, i64)| Weapon {
        base_item: row,
        delivery: Delivery::of_base_item(row).expect("a gun's row"),
        build: build::of(Some(wield), Some(size)),
        ..Weapon::UNARMED
    };
    let pistols = [12, 13, 14, 15, 16, 17].map(|row| (row, 4, 2));
    let long_guns = [18, 19, 20, 21, 22, 77]
        .map(|row| (row, 5, 4))
        .into_iter()
        .chain([(23, 6, 4), (24, 6, 4)]);
    let cases = pistols
        .into_iter()
        .map(|row| (row, true))
        .chain(long_guns.map(|row| (row, false)));
    for (cells, one_handed) in cases {
        let mut sheet = Sheet::dev_soldier();
        sheet.weapons = [Some(gun(cells)), None];
        let row = cells.0;
        let Held::Blaster(held) = Kit::of(&sheet).held(0) else {
            panic!("baseitems.2da row {row} is not held as a gun");
        };
        assert_eq!(held.one_handed, one_handed, "baseitems.2da row {row}");
        let (ready, shot) = if one_handed {
            (Anim::TORSO_WEAPONREADY2, Anim::BOTH_ATTACK2)
        } else {
            (Anim::TORSO_WEAPONREADY3, Anim::BOTH_ATTACK3)
        };
        assert_eq!(gun_ready(held.one_handed), ready, "row {row}'s ready pose");
        assert_eq!(gun_shot(held.one_handed), shot, "row {row}'s shot");
    }
    // An ion blaster in the right hand and a blaster pistol in the left.
    let (ion, pistol) = (gun((15, 4, 2)), gun((12, 4, 2)));
    assert!(sim::inventory::pairs(&ion, &pistol), "the two pistols pair");
    let mut sheet = Sheet::dev_soldier();
    sheet.weapons = [Some(ion), Some(pistol)];
    assert_eq!(sheet.wield_number(), 6, "two pistols' wield number");
    let kit = Kit::of(&sheet);
    for hand in 0..2 {
        assert!(
            matches!(kit.held(hand), Held::Blaster(gun) if gun.one_handed),
            "hand {hand} holds its pistol in one hand"
        );
    }
}

/// A feat attack chosen with the next-feat key (`combat::picks`): a gun
/// goes round the lines known, each type keeping its own; two blades and a
/// double-bladed weapon carry each melee line known in turn, then none, and
/// a blow carries the chosen feat (Flurry makes two blades, a staff or
/// fists attack faster, and multiplies no blow's damage); a single
/// blade has no choice; a script's talent chooses too; the choices come
/// back from their bytes. The sim alone. [approved on 2026-11-16's approval]
#[test]
fn feat_picks() {
    probed("feat picks", &sim::picks_probes::probes(), 8);
}

/// The action bar on the sheet (`combat::bar`). A content put in a slot
/// moves there, and swaps places with what the slot held if it was already
/// on the bar. A slot past the bar's end is refused, and clearing a slot
/// empties it. The bar comes back whole from its bytes, and a sheet from a
/// snapshot older than the bar reads an empty bar. A bar from a snapshot
/// older than the ten slots reads its six slots, and slots 7 to 10 are
/// empty. The HUD shows the first six slots, and the slots after them up to
/// the last one that holds something. The bar's first fill
/// puts attack powers, feat attacks that fit the weapon in hand, grenades
/// and medpacs in the first four slots. A power learned later takes an
/// empty slot or the slot of its lower tier. The host fills the bar, and a
/// client leaves it to the host. The power wheel lists offensive powers
/// first. It leaves out what the player hid, lower tiers, and what is on
/// the bar, except an item kind of which the character carries two or more
/// different items. Mines are listed only with a rank in Demolitions. Choosing a feat
/// attack sets it for the weapon in hand. For blades and empty hands,
/// choosing it again turns it off. A gun always uses a line the character
/// knows, so choosing the line a gun uses already changes nothing. The bar
/// edit and feat attack actions come back whole from their
/// bytes. A seat edits only the bars of the party members it may control,
/// and puts on them only what the character may put there: no power it
/// does not know, and mines only when the party carries a kit. The sim
/// alone. [approved on 2026-11-16]
#[test]
fn action_bar_state() {
    probed("action bar", &sim::combat::bar_probes::probes(), 22);
}

/// The Force powers and feat attacks that the action bar and the power
/// wheel offer (issue 417). Every power that a menu of the game offers is
/// among them, including a power for the caster's own side picked at a
/// level-up and a power past row 255. The force button casts each of them
/// through its impact script. A power that no menu offers is left out.
/// With fists or a stun baton, the character chooses among the melee feat
/// attack lines, and each blow uses the line chosen. The sim alone.
/// [approved on 2026-11-16]
#[test]
fn power_and_feat_cycle() {
    probed(
        "power and feat cycle",
        &sim::power_cycle_probes::probes(),
        4,
    );
}

/// Armour barring Force powers (issue 428), as both games' executables
/// decide it when a cast begins from each `spells.2da` row's
/// `forbiditemmask` and `requireitemmask`: the power key with a barred
/// power selected casts and pays nothing, a queued cast of it (a script's,
/// an AI talent's) ends, and both run without the armour; an item in any
/// slot reaches the mask; a required item allows its power. The sim alone
/// (approved 2026-11-16).
#[test]
fn armour_bars_powers() {
    probed("armour bars powers", &sim::armour_force_probes::probes(), 4);
}

/// The duel rules (`kjk sim probes --only "duel rules"`): duels played from
/// trace scenarios under the JA, Movie Duels, Movie Duels Advanced and
/// Advanced rules. Blocks, Stamina, kicks, the companion the player leads
/// and the other rules each have their checks. The sim alone (proposed
/// 2026-11-17, awaiting approval).
#[test]
fn duel_rules() {
    probed("duel rules", &sim::trace::duel_probes::probes(), 71);
}

/// TSL's Force rules (`kjk sim probes --only "tsl force"`): the Forms, a
/// power's cost by alignment and the room's Force rating, Force point
/// regeneration, and the powers each class is granted. The sim alone
/// (proposed 2026-11-17, awaiting approval).
#[test]
fn tsl_force() {
    probed("tsl force", &sim::force_probes::probes(), 19);
}

/// Force Scream's ear covering (`kjk sim probes --only "force scream"`): a
/// foe that fails its save covers its ears for the rulebook's time and plays
/// JA's clips for it, one that makes the save does not, a droid never does,
/// the native save covers them too, and a second scream inside the immunity
/// does not cover them again. The sim alone (proposed 2026-11-17, awaiting
/// approval).
#[test]
fn force_scream_ears() {
    probed("force scream", &sim::scream_probes::probes(), 5);
}

/// The powers only their impact scripts cast (`kjk sim probes --only
/// "power rows"`): a scripted power from a foe's menu reaches as far as its
/// row's range and its script's own sphere decides whom it reaches, its
/// cast shows the row's clips, a creature casts one its script chose and
/// the native chooser picks one by its row; a sheet that knows power and
/// feat row 400 survives a snapshot, an old one loads; the Force routines
/// mods call are bound, and a call of an unbound routine warns once
/// (awaiting the owner's approval).
#[test]
fn power_rows() {
    probed("power rows", &sim::power_rows_probes::probes(), 8);
}

/// The talents a companion's combat script queues (`kjk sim probes --only
/// "companion talents"`). A companion's own AI drops every Force power it
/// queues as a talent, whether the script named the power or found it. A
/// power the script casts with `ActionCastSpellAtObject` is still cast. A
/// companion keeps a medpac, a feat, and a droid's own weapon. It keeps a
/// talent that another script queues for it. A droid drops its shield. An
/// enemy keeps every talent.
#[test]
fn companion_talents() {
    probed(
        "companion talents",
        &sim::companion_talent_probes::probes(),
        10,
    );
}

/// The talents an enemy's combat scripts queue (`kjk sim probes --only
/// "enemy talents"`). A Force power and a medpac that an enemy's generic
/// combat script queues are dropped, because the engine chooses them. A
/// feat and a droid's own weapon from the same script are kept. A power
/// from a script outside the list of generic combat scripts is kept, and
/// so is a power from a script that is not the enemy's AI hook. A power
/// from each boss script that the survey of 19 November 2026 found is
/// kept. (proposed 2026-11-19, awaiting approval)
#[test]
fn enemy_talents() {
    probed(
        "enemy talents",
        &sim::companion_talent_probes::enemy_probes(),
        7,
    );
}

/// The Force powers and grenades a companion chooses itself (`kjk sim
/// probes --only "companion choices"`). A companion heals a hurt ally. It
/// throws a grenade only at a group of enemies, and never one of the
/// party's last two grenades. It waits between uses. It leaves medpacs to
/// its own script and uses no stim. Two companions together never spend
/// the party's last two grenades. A companion chooses nothing while a
/// conversation holds the world, or while the player leads it. A feat
/// that its script picks every round does not stop it from healing. It
/// casts Revitalize only when a member of the party has fallen. It fires
/// no rocket without a wrist launcher. It casts Energy Resistance against a
/// blaster, only the highest rank it knows, and no buff against a
/// vibroblade. It casts Force Barrier against
/// a vibroblade. It does not cast a buff whose effect already runs on it.
#[test]
fn companion_choices() {
    probed("companion choices", &sim::choice_probes::probes(), 22);
}

/// The Force powers, items and grenades an enemy chooses itself (`kjk sim
/// probes --only "enemy choices"`). An enemy casts a disabling power on a
/// target that is not disabled. It throws a grenade from its own pack at
/// two members of the party standing together, and the party keeps all of
/// its own grenades. It throws its last grenade. It raises its own energy
/// shield against a player with a blaster. Below half health it uses the
/// weakest medpac that restores all its missing hit points, or a stronger
/// one when the weakest would not. A neutral creature beside the fight uses
/// nothing. An enemy uses no medpac when a script ends its fight on the
/// step on which it chose one. (proposed 2026-11-19, awaiting approval)
#[test]
fn enemy_choices() {
    probed("enemy choices", &sim::enemy_choice_probes::probes(), 8);
}

/// The waits that the characters of one side keep for each other
/// (`sim::combat::choose::side_allows`, `ai.choices.side_wait_ms`). A
/// character may use a grenade, an area power, a disable, a heal or a buff
/// only when no ally it can see used the same kind within that kind's
/// wait. Each kind is refused 1 ms before its wait ends, and allowed when
/// it ends and when no ally used it. Four enemies with grenades facing two
/// members of the party standing together throw at least twice in 30
/// seconds, and never twice within the grenade's wait. Two companions who
/// know Heal, beside a badly hurt player, heal at least twice in 30
/// seconds, and never twice within the heal's wait. (proposed 2026-11-19,
/// awaiting approval)
#[test]
fn choice_side_waits() {
    use sim::combat::choose::{SideKind, side_allows, side_wait_ms};
    let used = 1_000;
    for &kind in SideKind::ALL {
        let ends = used + side_wait_ms(kind);
        for (now, last, expected) in [
            (ends - 1, Some(used), false),
            (ends, Some(used), true),
            (used, None, true),
        ] {
            assert_eq!(
                side_allows(kind, now, last),
                expected,
                "{kind:?} at {now} ms after an ally's use at {last:?}"
            );
        }
    }
    probed(
        "enemy side waits",
        &sim::enemy_choice_probes::side_wait_probes(),
        1,
    );
    probed(
        "companion side waits",
        &sim::choice_probes::side_wait_probes(),
        1,
    );
}

/// Powers the combat AI leaves out because they would do nothing
/// (`sim::combat::choose::does_something`). Each row is one case. Force
/// Scream does nothing to a target that cannot be made to cover its ears.
/// Force Suppression does nothing to a target whose weakest running power
/// is too strong for it. Neither dispel does anything to a target with no
/// power running. Beast Trick and Beast Confusion do nothing to a target
/// that is not a beast. The AI also leaves out Rage below half health
/// (`wise_for`), and a lower rank of a line whose higher rank the
/// character knows (`outranked`).
#[test]
fn choice_effects() {
    use sim::combat::choose::{Mark, does_something};
    use sim::combat::spells::{
        FORCE_POWER_BEAST_CONFUSION, FORCE_POWER_BEAST_TRICK, FORCE_POWER_FORCE_BREACH,
        FORCE_POWER_FORCE_SCREAM, FORCE_POWER_STUN, FORCE_POWER_SUPRESS_FORCE,
    };
    let deaf = Mark {
        deaf: true,
        ..Mark::default()
    };
    let buffed = |level: u8| Mark {
        buff: Some(level),
        ..Mark::default()
    };
    let beast = Mark {
        beast: true,
        ..Mark::default()
    };
    let rows = [
        (
            "scream on a hearing target",
            FORCE_POWER_FORCE_SCREAM,
            Mark::default(),
            true,
        ),
        (
            "scream on a deaf target",
            FORCE_POWER_FORCE_SCREAM,
            deaf,
            false,
        ),
        ("stun on a deaf target", FORCE_POWER_STUN, deaf, true),
        (
            "suppression on a level 2 power",
            FORCE_POWER_SUPRESS_FORCE,
            buffed(2),
            true,
        ),
        (
            "suppression on a level 3 power",
            FORCE_POWER_SUPRESS_FORCE,
            buffed(3),
            false,
        ),
        (
            "suppression on no power",
            FORCE_POWER_SUPRESS_FORCE,
            Mark::default(),
            false,
        ),
        (
            "breach on a level 3 power",
            FORCE_POWER_FORCE_BREACH,
            buffed(3),
            true,
        ),
        (
            "breach on no power",
            FORCE_POWER_FORCE_BREACH,
            Mark::default(),
            false,
        ),
        (
            "beast trick on a beast",
            FORCE_POWER_BEAST_TRICK,
            beast,
            true,
        ),
        (
            "beast trick on a person",
            FORCE_POWER_BEAST_TRICK,
            Mark::default(),
            false,
        ),
        (
            "beast confusion on a person",
            FORCE_POWER_BEAST_CONFUSION,
            Mark::default(),
            false,
        ),
        ("stun on a beast", FORCE_POWER_STUN, beast, true),
    ];
    for (what, row, mark, wanted) in rows {
        assert_eq!(does_something(row, mark), wanted, "{what}");
    }
    // Rage is wise only at `ai.choices.rage_above` of the maximum hit
    // points or more. Other powers are wise at any health.
    use sim::combat::choose::wise_for;
    use sim::combat::spells::{FORCE_POWER_FURY, FORCE_POWER_HEAL};
    let limit = rules::ai::choices::RAGE_ABOVE;
    let wise = [
        ("rage at the limit", FORCE_POWER_FURY, limit, true),
        (
            "rage below the limit",
            FORCE_POWER_FURY,
            limit - 0.01,
            false,
        ),
        ("heal below the limit", FORCE_POWER_HEAL, limit - 0.01, true),
    ];
    for (what, row, health, wanted) in wise {
        assert_eq!(wise_for(row, health), wanted, "{what}");
    }
    // A rank is outranked when the character knows a rank that lists it
    // as a prerequisite in `spells.2da`. Force Barrier (135) is needed by
    // Improved Force Barrier (136), which Master Force Barrier (137) needs.
    use sim::combat::choose::outranked;
    use sim::combat::spells::{
        FORCE_POWER_FORCE_BARRIER, FORCE_POWER_IMPROVED_FORCE_BARRIER,
        FORCE_POWER_MASTER_FORCE_BARRIER,
    };
    let (barrier, improved, master) = (
        FORCE_POWER_FORCE_BARRIER,
        FORCE_POWER_IMPROVED_FORCE_BARRIER,
        FORCE_POWER_MASTER_FORCE_BARRIER,
    );
    // Master Energy Resistance (133) needs Resist Energy 2 (40) and Resist
    // Energy 1 (42). `force::of_row` orders only Resist Energy 1 and 2.
    // Mass Stasis (44) needs Stun (46) and Stasis (29), and `force::of_row`
    // puts all three on one line of the same level.
    use sim::combat::spells::{
        FORCE_POWER_MASS_STASIS, FORCE_POWER_MASTER_ENERGY_RESISTANCE, FORCE_POWER_RESIST_ENERGY_1,
        FORCE_POWER_RESIST_ENERGY_2, FORCE_POWER_STASIS,
    };
    let (energy_1, energy_2, energy_master) = (
        FORCE_POWER_RESIST_ENERGY_1,
        FORCE_POWER_RESIST_ENERGY_2,
        FORCE_POWER_MASTER_ENERGY_RESISTANCE,
    );
    let (stasis, mass_stasis) = (FORCE_POWER_STASIS, FORCE_POWER_MASS_STASIS);
    let needs = std::collections::BTreeMap::from([
        (improved, [Some(barrier), None]),
        (master, [Some(improved), None]),
        (energy_2, [Some(energy_1), None]),
        (energy_master, [Some(energy_2), Some(energy_1)]),
        (stasis, [Some(FORCE_POWER_STUN), None]),
        (mass_stasis, [Some(FORCE_POWER_STUN), Some(stasis)]),
    ]);
    let all_energy = [energy_1, energy_2, energy_master];
    let ranks = [
        (
            "the first rank, the second known",
            barrier,
            &[barrier, improved][..],
            true,
        ),
        (
            "the second rank, the third not known",
            improved,
            &[barrier, improved][..],
            false,
        ),
        (
            "the second rank, the third known",
            improved,
            &[barrier, improved, master][..],
            true,
        ),
        (
            "the highest rank",
            master,
            &[barrier, improved, master][..],
            false,
        ),
        ("the only rank known", barrier, &[barrier][..], false),
        (
            "resist energy 2, master energy resistance known",
            energy_2,
            &all_energy[..],
            true,
        ),
        (
            "resist energy 1, master energy resistance known",
            energy_1,
            &all_energy[..],
            true,
        ),
        (
            "master energy resistance",
            energy_master,
            &all_energy[..],
            false,
        ),
        (
            "stasis, mass stasis known on the same line",
            stasis,
            &[FORCE_POWER_STUN, stasis, mass_stasis][..],
            false,
        ),
    ];
    for (what, row, known, wanted) in ranks {
        assert_eq!(
            outranked(row, |r| known.contains(&r), &needs),
            wanted,
            "{what}"
        );
    }
    // The chooser offers only the highest rank a character knows of each
    // line (`highest_known`). A character who knows all three energy
    // resistance powers is offered only Master Energy Resistance. Stun,
    // Stasis and Mass Stasis are one level of one mapped line, so all three
    // are offered, as before.
    use sim::combat::choose::highest_known;
    let knowing = |rows: &[u16]| {
        rows.iter()
            .fold(sim::combat::sheet::Powers::default(), |powers, row| {
                powers.with(*row)
            })
    };
    let stuns = [FORCE_POWER_STUN, stasis, mass_stasis];
    let highest = [
        ("resist energy 1 of three", energy_1, &all_energy[..], false),
        ("resist energy 2 of three", energy_2, &all_energy[..], false),
        (
            "master energy resistance",
            energy_master,
            &all_energy[..],
            true,
        ),
        (
            "resist energy 2 without the master rank",
            energy_2,
            &[energy_1, energy_2][..],
            true,
        ),
        (
            "resist energy 1 under resist energy 2",
            energy_1,
            &[energy_1, energy_2][..],
            false,
        ),
        ("stasis among the stun line", stasis, &stuns[..], true),
        (
            "barrier under improved barrier",
            barrier,
            &[barrier, improved][..],
            false,
        ),
    ];
    for (what, row, known, wanted) in highest {
        assert_eq!(
            highest_known(row, &knowing(known), Some(&needs)),
            wanted,
            "{what}"
        );
    }
    // An effect of power `spell` shows that power `row` runs when `spell`
    // is `row` or a higher rank of its line. Inspire Followers IV (170)
    // needs III (169), which needs I (167). Without prerequisites, only the
    // same row counts.
    use sim::combat::choose::covers;
    let inspire = std::collections::BTreeMap::from([
        (168, [Some(167), None]),
        (169, [Some(167), Some(168)]),
        (170, [Some(169), None]),
    ]);
    let lines = [
        ("the same rank", barrier, barrier, Some(&needs), true),
        ("a higher rank", master, barrier, Some(&needs), true),
        ("a lower rank", barrier, master, Some(&needs), false),
        ("another line", energy_master, barrier, Some(&needs), false),
        ("two ranks up the line", 170, 167, Some(&inspire), true),
        (
            "no prerequisites, a higher rank",
            master,
            barrier,
            None,
            false,
        ),
        (
            "no prerequisites, the same rank",
            barrier,
            barrier,
            None,
            true,
        ),
    ];
    for (what, spell, row, prerequisites, wanted) in lines {
        assert_eq!(covers(spell, row, prerequisites), wanted, "{what}");
    }
}

/// The reasons a buff fits the fight (`ai.choices.reasons`,
/// `sim::combat::choose::fits`). No row has two reasons, every listed row
/// is a buff, and every buff that no reason lists is always fit. Each row
/// of the second table is one reason, true and false.
#[test]
fn choice_reasons() {
    use sim::combat::choose::{Choice, Foes, ROWS_BY_REASON, Reason, Scene, fits, reason_of};
    let buffs = rules::ai::choices::kinds::BUFF;
    let mut seen = std::collections::BTreeMap::new();
    for (reason, rows) in ROWS_BY_REASON {
        for &row in rows {
            assert!(
                buffs.contains(&row),
                "{reason:?} lists row {row}, which is not a buff"
            );
            if let Some(other) = seen.insert(row, reason) {
                panic!("row {row} is both {other:?} and {reason:?}");
            }
            assert_eq!(reason_of(Choice::Power(row)), reason);
        }
    }
    for row in buffs.iter().filter(|row| !seen.contains_key(row)) {
        assert_eq!(reason_of(Choice::Power(*row)), Reason::Always, "row {row}");
    }
    let calm = Scene {
        now: 1000,
        fight_ms: 1000,
        intense: false,
        companion: true,
        own_health: 1.0,
        lowest_ally_health: 1.0,
        own_droid: false,
        own_blade: false,
        foes: Foes::default(),
        intense_buff: false,
        target: None,
    };
    let facing = |foes: Foes| Scene { foes, ..calm };
    let energy = Foes {
        energy: true,
        ..Foes::default()
    };
    let physical = Foes {
        physical: true,
        ..Foes::default()
    };
    let force = Foes {
        force: true,
        ..Foes::default()
    };
    let rows = [
        (Reason::Energy, facing(energy), true),
        (Reason::Energy, facing(physical), false),
        (Reason::Physical, facing(physical), true),
        (Reason::Physical, facing(force), false),
        (Reason::ForceUsers, facing(force), true),
        (Reason::ForceUsers, facing(energy), false),
        (
            Reason::Intense,
            Scene {
                intense: true,
                ..calm
            },
            true,
        ),
        (Reason::Intense, calm, false),
        (
            Reason::Blade,
            Scene {
                own_blade: true,
                ..calm
            },
            true,
        ),
        (Reason::Blade, calm, false),
        (Reason::Always, calm, true),
    ];
    for (reason, scene, wanted) in rows {
        assert_eq!(fits(reason, &scene), wanted, "{reason:?} in {scene:?}");
    }
}

/// The grenades a character has used or is about to throw that the party's
/// inventory still counts (`sim::combat::items::Pack::not_yet_taken`). A
/// companion subtracts its own and every other member's from what the
/// inventory shows. The companion keeps the party's last grenades only
/// when this count is right. Each row is one state of the pack. The rows
/// are nothing pending, a throw queued, a throw charging, a use not yet
/// taken, and a use of another row.
#[test]
fn grenades_not_yet_taken() {
    use sim::combat::Sheet;
    use sim::combat::items::Pack;
    const FRAG: u16 = 87;
    const THERMAL: u16 = 89;
    let sheet = Sheet::dev_soldier();
    let at = |row: u16| {
        sheet
            .items
            .iter()
            .position(|carried| carried.is_some_and(|carried| carried.row == row))
            .expect("the dev soldier carries the row")
    };
    let used = |row: u16| {
        let mut pack = Pack::default();
        pack.used[at(row)] = 1;
        pack
    };
    let rows = [
        ("nothing pending", Pack::default(), 0),
        (
            "a frag queued",
            Pack {
                queued: Some(FRAG),
                ..Pack::default()
            },
            1,
        ),
        (
            "a frag charging",
            Pack {
                charging_from: Some(1000),
                ..Pack::default()
            },
            1,
        ),
        ("a frag used, not yet taken", used(FRAG), 1),
        ("a thermal used", used(THERMAL), 0),
    ];
    for (what, pack, wanted) in rows {
        assert_eq!(pack.not_yet_taken(&sheet, FRAG, 2000), wanted, "{what}");
    }
}

/// A queued grenade row that cannot be thrown is forgotten
/// (`sim::combat::grenades::pull`), so that it never counts as a coming
/// throw forever. Each row is one pack. A rocket queued by a character
/// without a wrist launcher is forgotten. A frag queued by a character with
/// no frag left is forgotten. A frag queued by a character who carries one
/// stays queued, and so does a rocket queued by a character wearing a
/// launcher.
#[test]
fn unthrowable_grenades_forgotten() {
    use sim::combat::grenades::{pull, usable};
    use sim::combat::items::{Carried, Pack};
    const FRAG: u16 = 87;
    const ROCKET: u16 = 185;
    let carried = |row: u16| Carried { row, count: 1 };
    let items = [Some(carried(FRAG)), Some(carried(ROCKET)), None, None];
    let only_rockets = [Some(carried(ROCKET)), None, None, None];
    let rows = [
        ("a rocket, no launcher", ROCKET, items, false, None),
        ("a frag, none carried", FRAG, only_rockets, true, None),
        ("a frag, one carried", FRAG, items, false, Some(FRAG)),
        (
            "a rocket, a launcher worn",
            ROCKET,
            items,
            true,
            Some(ROCKET),
        ),
    ];
    for (what, row, items, launcher, wanted) in rows {
        let mut pack = Pack {
            queued: Some(row),
            ..Pack::default()
        };
        let throwable = usable(&items, launcher);
        let thrown = pull(&mut pack, (&throwable, None), 0, false, 1000);
        assert!(
            thrown.is_none(),
            "{what}: nothing is thrown without a press"
        );
        assert_eq!(pack.queued, wanted, "{what}");
    }
}

/// The chooser's order of preference (`sim::combat::choose::ranked`). Its
/// first entry is what `choose` picks. A passing heal comes first, and the
/// rest go from the highest score down. An option that does not pass is
/// never in the list.
#[test]
fn choice_ranks() {
    use sim::combat::choose::{Choice, Kind, Option_, Scene, Target, choose, ranked};
    let option = |choice, kind| Option_ {
        choice,
        kind,
        droids_only: false,
        running: false,
    };
    let target = Target {
        in_reach: false,
        disabled: true,
        staggered: false,
        droid: false,
        hostiles_near: 3,
        allies_near: 0,
    };
    let scene = Scene {
        now: 60_000,
        fight_ms: 60_000,
        intense: true,
        companion: true,
        own_health: 1.0,
        lowest_ally_health: 0.2,
        own_droid: false,
        own_blade: false,
        foes: sim::combat::choose::Foes::default(),
        intense_buff: false,
        target: Some(target),
    };
    let options = [
        option(Choice::Power(35), Kind::Damage),
        option(Choice::Grenade(87), Kind::AreaDamage),
        option(Choice::Power(46), Kind::Disable),
        option(Choice::Power(28), Kind::Heal),
    ];
    for pick in [0.0, 0.5, 0.99] {
        let order = ranked(&options, &scene, pick);
        assert_eq!(order.first().copied(), choose(&options, &scene, pick));
        assert_eq!(order[0], Choice::Power(28), "the heal comes first");
        assert_eq!(
            order.len(),
            3,
            "the disable on a disabled target is left out"
        );
        assert!(order.contains(&Choice::Power(35)) && order.contains(&Choice::Grenade(87)));
    }
}

/// KOTOR 2's Force powers in KOTOR 1 (`kjk::probes::kotor2_powers`, `kjk
/// sim probes --only "kotor2 powers"`): the 23 rows KOTOR 1 gets with KOTOR
/// 2's costs, names, icons and levels, KOTOR 1's own data and rules unchanged, a
/// mod's row 132 with KOTOR 2's rows lent after it, and a KOTOR 1 Jedi casting
/// Force Barrier, Battle Meditation and Force Scream through KOTOR 2's
/// script. Needs `--features rig` and both installs.
/// (proposed 2026-11-17, awaiting approval)
#[test]
fn kotor2_powers_in_kotor1() {
    #[cfg(not(feature = "rig"))]
    println!("kotor2_powers_in_kotor1 skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("kotor2 powers", &kjk::probes::kotor2_powers(), 11);
}

/// KOTOR 2's feats in KOTOR 1 (`kjk::probes::kotor2_feats`, `kjk sim
/// probes --only "kotor2 feats"`): the 39 rows KOTOR 1 gets with KOTOR 2's
/// names, icons, picks and grants, the Class Skill feats by KOTOR 1's
/// skills, KOTOR 1's own rows unchanged, a mod's row 125 with KOTOR 2's
/// feats lent after it, missed grants caught up at the next level, and each new
/// feat rule with and without its feat, Dual Strike in a scene too. Needs
/// `--features rig` and both installs.
/// (proposed 2026-11-17, awaiting approval)
#[test]
fn kotor2_feats_in_kotor1() {
    #[cfg(not(feature = "rig"))]
    println!("kotor2_feats_in_kotor1 skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("kotor2 feats", &kjk::probes::kotor2_feats(), 18);
}

/// KOTOR 1 past level 20 (`kjk::probes::kotor1_level_50`, `kjk sim probes
/// --only "kotor1 level 50"`): every KOTOR 1 class's tables to level 50 with
/// KOTOR 1's own rows for levels 1 to 20 kept, each class levelled from 20
/// to 21 and from 20 to 30, Master Revitalize at level 21, and KOTOR 2's
/// tables unchanged. Needs `--features rig` and both installs.
/// (proposed 2026-11-17, awaiting approval)
#[test]
fn kotor1_level_50() {
    #[cfg(not(feature = "rig"))]
    println!("kotor1_level_50 skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("kotor1 level 50", &kjk::probes::kotor1_level_50(), 5);
}

/// Rows by label (`kjk::probes::rows_by_label`, `kjk sim probes --only "rows
/// by label"`): in the sim, a mod's script and a lent script each get their
/// own row numbers from the routines, a sheet keeps canonical numbers and
/// survives a snapshot; on both games' data, the identity without mods, a
/// made-up mod's named rows at the engine's numbers and its own rows past
/// KOTOR 2's, KOTOR 2's rows lent after the mod's skipping its labels,
/// prerequisites, a creature template and an item naming install rows, the
/// action bar's icon, the same map twice, lent scripts told apart, a feat of
/// a mod's own and a replaced `k_sp1_generic`. Needs `--features rig` and
/// both installs for the second part. (proposed 2026-11-17, awaiting approval)
#[test]
fn rows_by_label() {
    #[cfg(not(feature = "rig"))]
    println!("rows_by_label skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("rows by label", &kjk::probes::rows_by_label(), 15);
}

/// Real Force power mods (`kjk::probes::real_mods`, `kjk sim probes --only
/// "real mods"`) on game folders a patcher patched, named by
/// `KJK_MODDED_KOTOR2` (High-Level Force Powers 2.0 and 2.1, Powers of the
/// Triumvirate, or both) and `KJK_MODDED_KOTOR1` (K2 Force Powers for K1,
/// Bastila Has TSL Battle Meditation). Without either variable it is
/// skipped and says so; a check about a mod the folder lacks is left out and
/// says so. Needs `--features rig`. (proposed 2026-11-17, awaiting approval)
#[test]
fn real_mods() {
    #[cfg(not(feature = "rig"))]
    println!("real_mods skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    {
        let named = ["KJK_MODDED_KOTOR2", "KJK_MODDED_KOTOR1"]
            .into_iter()
            .filter(|var| std::env::var(var).is_ok_and(|path| !path.trim().is_empty()))
            .count();
        if named == 0 {
            println!(
                "real_mods skipped: KJK_MODDED_KOTOR2 and KJK_MODDED_KOTOR1 name no game folders"
            );
            return;
        }
        // A folder with any of the mods gives at least three checks (the
        // rows, the level-up and the creatures' menus); the checks of a
        // mod it lacks are left out and named on the standard error.
        probed("real mods", &kjk::probes::real_mods(), 3 * named);
    }
}

/// TSL's workbench, lab station and upgrade bench (`kjk sim probes --only
/// workbench`): breakdowns and the yield by skill, items made, refusals with
/// the game's lines, components as a count, and the upgrade bench's
/// Assemble. The sim alone (proposed 2026-11-17, awaiting approval).
#[test]
fn workbench() {
    probed("workbench", &sim::workshop_probes::probes(), 16);
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
/// The `led_own_walked_by_script` check was added and approved on
/// 2026-11-12. A script's move of the player's own character walks it while
/// a companion leads, as Hendar's gate in K1's Undercity needs.
#[test]
fn guests_drive_companions() {
    probed("drivers", &sim::drivers::probes(), 30);
}

/// Conversations in co-op (I31): `sim::coop_talk::probes`, every phase's
/// cases and the whole scenario run twice ending alike; the sim alone.
#[test]
fn dialogue_coop() {
    probed("coop talk", &sim::coop_talk::probes(), 145);
}

/// Scene mode (`docs/DIALOGUE.md`): the scene probe group,
/// `sim::scene_probes::all` (`kjk sim probes scene`), each case on the
/// proving ground with conversations and scripts written there; the sim
/// alone. Approved by the owner 2026-10-17 (do what you think is best).
/// The two saber-rules cases (a scene plays alike under JA, MD and MDA)
/// were approved 2026-11-09. The check that a loaded game plays the
/// area's music as placed (`load_plays_music_as_placed`) was approved
/// 2026-11-12. The check that a random walk beside a shut door never opens
/// it (`random_walk_keeps_door_shut`, #448) was added and approved
/// 2026-11-15. The check that a weapon a script equips in a party member's
/// hand during its conversation stays out (`equipped_in_talk_stays_out`,
/// issue 392) was approved 2026-11-16. The check that a TSL animation row
/// marked looping, played with a duration of -1, plays on
/// (`looping_row_plays_on`, issue 490) awaits approval. The check that a
/// held party member's scripted walk to a door ends at the door's action
/// point, its queued opening opens the door, and its walk goes on after
/// the conversation ends (`held_walk_opens_door`, issue 489) replaces the
/// check that such an opening failed (`party_queue_cannot_open`), and
/// awaits approval. The check that a forced move to a point of a shut
/// door's doorway, short of the panel, arrives there and leaves the door
/// shut (`walks_into_doorway_short_of_door`, issue 501) awaits approval.
#[test]
fn scene_mode() {
    probed("scene", &sim::scene_probes::all(), 78);
}

/// The party's calls (I31): `sim::party::probes`, among them in combat,
/// a break's freeze and the ready check, and what enters a trigger; the sim
/// alone.
#[test]
fn party_calls() {
    probed("party", &sim::party::probes(), 20);
}

/// Which companions a player may choose (`sim::script::party_choice`),
/// shared by the co-op lobby and the party selection screen. Each case runs
/// through both entry points.
#[test]
fn party_choice() {
    probed("party choice", &sim::script::party_choice::probes(), 7);
}

/// The co-op lobby's rules (`session::lobby`): free picks, swaps, refused
/// picks, required companions, a guest leaving, a new save, and a save
/// with no companions.
#[test]
fn coop_lobby() {
    probed("lobby", &session::lobby::probes(), 14);
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

/// A script makes a waypoint from its template, and a party that leaves the
/// module and comes back by the waypoint's tag arrives at it, not at the
/// module's entry: `sim::made_waypoint_probes` (TSLRCM's Red Eclipse scene
/// on the Nar Shaddaa landing pad, issue 519).
#[test]
fn made_waypoint_is_an_arrival() {
    probed("made_waypoint", &sim::made_waypoint_probes::probes(), 2);
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

/// Companions fighting round a player standing in their way, the host's
/// and a guest's, go round it or wait and never shove it:
/// `sim::push_probes` (Discord report of 2026-11-11; proposed and approved
/// 2026-11-12).
#[test]
fn companions_do_not_shove_players() {
    probed("push", &sim::push_probes::probes(), 3);
}

/// TSL's Force Deflection and Force Redirection are never cast, and they
/// let a character without a lightsaber turn blaster bolts, Redirection
/// back at the shooter: `sim::trace::deflection_probes` (GitHub #425 and
/// a Discord report of 2026-11-14; proposed 2026-11-14, approved
/// on 2026-11-16).
#[test]
fn force_deflection_turns_bolts() {
    probed("deflection", &sim::trace::deflection_probes::probes(), 7);
}

/// Under the Advanced rules a kick needs and uses `duel.kicks.uses`
/// Stamina, a fighter with full Stamina starts no more than 2 kicks in a
/// row, and a player knocked down by a kick is not knocked down by another
/// for `duel.kicks.player_guard_ms`: `sim::trace::duel_probes::kick_probes`
/// (proposed 2026-11-17, awaiting the owner's approval).
#[test]
fn kick_balance() {
    probed("kick", &sim::trace::duel_probes::kick_probes(), 4);
}

/// The party members nobody plays follow and fight beside the body played:
/// `sim::party_fights_probes`, owner's request. The foe carries no items
/// (changed 2026-11-19, awaiting approval).
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
/// scenario's checks and two runs ending alike; the sim alone. The
/// `wrong_class_told_its_guest` check was changed and approved on
/// 2026-11-12. The notice its guest is told now names the reason, a class
/// its character has not taken up.
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

/// The rulebook's kinds of `spells.2da` rows (`ai.choices.kinds`). Every
/// listed row is a row the engine names, and no row is in two kinds. Every
/// row that works only on droids also has a kind. A row that is never
/// chosen has no kind and no reason.
#[test]
fn choice_kinds() {
    use sim::combat::spells::{NAMED, ROWS_BY_KIND, kind_of_row};
    let mut seen = std::collections::BTreeMap::new();
    for (kind, rows) in ROWS_BY_KIND {
        for &row in rows {
            assert!(
                NAMED.iter().any(|&(_, named)| named == row),
                "{kind:?} lists row {row}, which no label names"
            );
            if let Some(other) = seen.insert(row, kind) {
                panic!("row {row} is both {other:?} and {kind:?}");
            }
            assert_eq!(kind_of_row(row).map(|(found, _)| found), Some(kind));
        }
    }
    // A row that is never chosen has no kind and no reason.
    for row in rules::ai::choices::kinds::NEVER {
        assert!(
            !seen.contains_key(&row),
            "row {row} is never chosen but has a kind"
        );
        let reasons = sim::combat::choose::ROWS_BY_REASON;
        assert!(
            reasons.iter().all(|(_, rows)| !rows.contains(&row)),
            "row {row} is never chosen but has a reason"
        );
    }
    for row in rules::ai::choices::kinds::DROIDS_ONLY {
        assert_eq!(
            kind_of_row(row).map(|(_, droids)| droids),
            Some(true),
            "droids-only row {row} has no kind"
        );
    }
}

/// How the combat AI scores what a character can use
/// (`sim::combat::choose`). Each row names one rule of the chooser. A row
/// expects a score of exactly 0, or a score that passes the threshold.
#[test]
fn choice_scores() {
    use sim::combat::choose::{Choice, Foes, Kind, Option_, Scene, Target, choose, kind_of, score};
    const STUN: u16 = 46;
    const DROID_STUN: u16 = 47;
    const LIGHTNING: u16 = 35;
    const STORM: u16 = 25;
    const HEAL: u16 = 28;
    const VALOR: u16 = 22;
    const AURA: u16 = 18;
    const RESIST_ENERGY: u16 = 42;
    const BARRIER: u16 = 135;
    const RESIST_FORCE: u16 = 41;
    const SPEED: u16 = 8;
    const BATTLE_MEDITATION: u16 = 138;
    const CAMOUFLAGE: u16 = 156;
    const BATTLE_PRECOGNITION: u16 = 179;
    const BREATH_CONTROL: u16 = 270;
    const MEDPAC: u16 = 64;
    const STIM: u16 = 69;
    const FRAG: u16 = 87;
    const ION_BLAST: u16 = 274;
    const DROID_DESTROY: u16 = 13;
    const REPAIR_KIT: u16 = 84;
    const REVITALIZE: u16 = 173;
    const UNLISTED: u16 = 9999;
    const AFFECT_MIND: u16 = sim::combat::spells::FORCE_POWER_AFFECT_MIND;
    #[derive(Debug, PartialEq)]
    enum Expect {
        Scores,
        Zero,
    }
    use Expect::{Scores, Zero};
    let offer = |choice: Choice| {
        let (Choice::Power(row) | Choice::Item(row) | Choice::Grenade(row)) = choice;
        let (kind, droids_only) = kind_of(row, None).expect("a listed row");
        Option_ {
            choice,
            kind,
            droids_only,
            running: false,
        }
    };
    let running = |choice: Choice| Option_ {
        running: true,
        ..offer(choice)
    };
    let unlisted_buff = Option_ {
        choice: Choice::Power(UNLISTED),
        kind: Kind::Buff,
        droids_only: false,
        running: false,
    };
    let foe = Target {
        in_reach: false,
        disabled: false,
        staggered: false,
        droid: false,
        hostiles_near: 0,
        allies_near: 0,
    };
    let pair = Target {
        hostiles_near: 1,
        ..foe
    };
    let enemy = Scene {
        now: 60_000,
        fight_ms: 20_000,
        intense: false,
        companion: false,
        own_health: 1.0,
        lowest_ally_health: 1.0,
        own_droid: false,
        own_blade: false,
        foes: Foes::default(),
        intense_buff: false,
        target: Some(foe),
    };
    // A scene in the opening of a fight, `fight_ms` after it began.
    let opening = |fight_ms: u64| Scene { fight_ms, ..enemy };
    let facing = |foes: Foes| Scene {
        foes,
        ..opening(1000)
    };
    let companion = Scene {
        companion: true,
        ..enemy
    };
    let at = |target: Target| Scene {
        target: Some(target),
        ..enemy
    };
    let hurt = |health: f32| Scene {
        lowest_ally_health: health,
        ..enemy
    };
    let hurt_self = |health: f32, own_droid: bool| Scene {
        own_health: health,
        lowest_ally_health: health,
        own_droid,
        ..enemy
    };
    let rows: Vec<(&str, Option_, Scene, Expect)> = vec![
        (
            "grenade, one target",
            offer(Choice::Grenade(FRAG)),
            enemy,
            Zero,
        ),
        (
            "grenade, two targets, ally near",
            offer(Choice::Grenade(FRAG)),
            at(Target {
                allies_near: 1,
                ..pair
            }),
            Zero,
        ),
        (
            "grenade, two targets, calm, companion",
            offer(Choice::Grenade(FRAG)),
            Scene {
                target: Some(pair),
                ..companion
            },
            Zero,
        ),
        (
            "grenade, two targets, intense, companion",
            offer(Choice::Grenade(FRAG)),
            Scene {
                target: Some(pair),
                intense: true,
                ..companion
            },
            Scores,
        ),
        (
            "grenade, two targets, calm, enemy",
            offer(Choice::Grenade(FRAG)),
            at(pair),
            Scores,
        ),
        (
            "storm, one target",
            offer(Choice::Power(STORM)),
            enemy,
            Zero,
        ),
        (
            "storm, two targets",
            offer(Choice::Power(STORM)),
            at(pair),
            Scores,
        ),
        (
            "stun on a stunned target",
            offer(Choice::Power(STUN)),
            at(Target {
                disabled: true,
                ..foe
            }),
            Zero,
        ),
        (
            "stun on a standing target",
            offer(Choice::Power(STUN)),
            enemy,
            Scores,
        ),
        (
            "stun on a droid",
            offer(Choice::Power(STUN)),
            at(Target { droid: true, ..foe }),
            Zero,
        ),
        (
            "droid stun on a living target",
            offer(Choice::Power(DROID_STUN)),
            enemy,
            Zero,
        ),
        (
            "droid stun on a droid",
            offer(Choice::Power(DROID_STUN)),
            at(Target { droid: true, ..foe }),
            Scores,
        ),
        (
            "Droid Destroy style damage, droids only, living target",
            offer(Choice::Power(ION_BLAST)),
            enemy,
            Zero,
        ),
        (
            "Droid Destroy style damage, droids only, droid target, out of reach",
            offer(Choice::Power(ION_BLAST)),
            at(Target { droid: true, ..foe }),
            Scores,
        ),
        (
            "area damage, droids only, living cluster",
            offer(Choice::Power(DROID_DESTROY)),
            at(pair),
            Zero,
        ),
        (
            "lightning, target in reach",
            offer(Choice::Power(LIGHTNING)),
            at(Target {
                in_reach: true,
                ..foe
            }),
            Zero,
        ),
        (
            "lightning, target out of reach",
            offer(Choice::Power(LIGHTNING)),
            enemy,
            Scores,
        ),
        (
            "lightning, staggered target in reach",
            offer(Choice::Power(LIGHTNING)),
            at(Target {
                in_reach: true,
                staggered: true,
                ..foe
            }),
            Scores,
        ),
        (
            "lightning, no target",
            offer(Choice::Power(LIGHTNING)),
            Scene {
                target: None,
                ..enemy
            },
            Zero,
        ),
        (
            "heal, ally at 0.6",
            offer(Choice::Power(HEAL)),
            hurt(0.6),
            Zero,
        ),
        (
            "heal, ally at 0.45",
            offer(Choice::Power(HEAL)),
            hurt(0.45),
            Scores,
        ),
        (
            "heal, ally at 0.2",
            offer(Choice::Power(HEAL)),
            hurt(0.2),
            Scores,
        ),
        (
            "heal power, own health 0.9, ally at 0.2",
            offer(Choice::Power(HEAL)),
            Scene {
                own_health: 0.9,
                ..hurt(0.2)
            },
            Scores,
        ),
        (
            "medpac, enemy, own health 0.9, ally at 0.2",
            offer(Choice::Item(MEDPAC)),
            Scene {
                own_health: 0.9,
                ..hurt(0.2)
            },
            Zero,
        ),
        (
            "repair kit on a living character",
            offer(Choice::Item(REPAIR_KIT)),
            hurt_self(0.2, false),
            Zero,
        ),
        (
            "repair kit on a hurt droid",
            offer(Choice::Item(REPAIR_KIT)),
            hurt_self(0.2, true),
            Scores,
        ),
        (
            "medpac on a hurt droid",
            offer(Choice::Item(MEDPAC)),
            hurt_self(0.2, true),
            Zero,
        ),
        // Revitalize is offered only when a member of the party has fallen,
        // so it scores as a heal for a character with no hit points.
        (
            "revitalize, everyone at full health",
            offer(Choice::Power(REVITALIZE)),
            enemy,
            Scores,
        ),
        (
            "heal, ally at 0.2, no target",
            offer(Choice::Power(HEAL)),
            Scene {
                target: None,
                ..hurt(0.2)
            },
            Scores,
        ),
        // Force Aura's reason is always true. It keeps the opening's rule.
        (
            "buff at 6000 ms",
            offer(Choice::Power(AURA)),
            opening(6000),
            Zero,
        ),
        (
            "buff already running",
            running(Choice::Power(AURA)),
            opening(1000),
            Zero,
        ),
        (
            "buff at 1000 ms, no target",
            offer(Choice::Power(AURA)),
            Scene {
                target: None,
                ..opening(1000)
            },
            Scores,
        ),
        // A row from a mod that the reasons do not list behaves as Force
        // Aura does.
        (
            "unlisted buff at 1000 ms",
            unlisted_buff,
            opening(1000),
            Scores,
        ),
        (
            "unlisted buff at 6000 ms",
            unlisted_buff,
            opening(6000),
            Zero,
        ),
        // Each reason, true and false, in the opening.
        (
            "energy resistance, an enemy with energy damage",
            offer(Choice::Power(RESIST_ENERGY)),
            facing(Foes {
                energy: true,
                ..Foes::default()
            }),
            Scores,
        ),
        (
            "energy resistance, no enemy with energy damage",
            offer(Choice::Power(RESIST_ENERGY)),
            facing(Foes {
                physical: true,
                force: true,
                ..Foes::default()
            }),
            Zero,
        ),
        (
            "energy resistance, an enemy with energy damage, at 6000 ms",
            offer(Choice::Power(RESIST_ENERGY)),
            Scene {
                foes: Foes {
                    energy: true,
                    ..Foes::default()
                },
                ..opening(6000)
            },
            Zero,
        ),
        (
            "barrier, an enemy with physical damage",
            offer(Choice::Power(BARRIER)),
            facing(Foes {
                physical: true,
                ..Foes::default()
            }),
            Scores,
        ),
        (
            "barrier, no enemy with physical damage",
            offer(Choice::Power(BARRIER)),
            facing(Foes {
                energy: true,
                force: true,
                ..Foes::default()
            }),
            Zero,
        ),
        (
            "resist force, an enemy with Force powers",
            offer(Choice::Power(RESIST_FORCE)),
            facing(Foes {
                force: true,
                ..Foes::default()
            }),
            Scores,
        ),
        (
            "resist force, no enemy with Force powers",
            offer(Choice::Power(RESIST_FORCE)),
            facing(Foes {
                energy: true,
                physical: true,
                ..Foes::default()
            }),
            Zero,
        ),
        (
            "speed, holding a blade",
            offer(Choice::Power(SPEED)),
            Scene {
                own_blade: true,
                ..opening(1000)
            },
            Scores,
        ),
        (
            "speed, holding a blaster",
            offer(Choice::Power(SPEED)),
            opening(1000),
            Zero,
        ),
        // Valor's reason is an intense fight. It may be cast at any time
        // while the fight is intense.
        (
            "valor, intense, opening",
            offer(Choice::Power(VALOR)),
            Scene {
                intense: true,
                ..opening(1000)
            },
            Scores,
        ),
        (
            "valor, calm, opening",
            offer(Choice::Power(VALOR)),
            opening(1000),
            Zero,
        ),
        (
            "valor, intense, after the opening",
            offer(Choice::Power(VALOR)),
            Scene {
                intense: true,
                ..enemy
            },
            Scores,
        ),
        (
            "valor, calm, after the opening",
            offer(Choice::Power(VALOR)),
            enemy,
            Zero,
        ),
        // After the opening, an intense buff is offered only while no
        // intense buff runs on the character, such as Valor.
        (
            "battle meditation, intense, after the opening, valor running",
            offer(Choice::Power(BATTLE_MEDITATION)),
            Scene {
                intense: true,
                intense_buff: true,
                ..enemy
            },
            Zero,
        ),
        (
            "battle meditation, intense, after the opening, no intense buff running",
            offer(Choice::Power(BATTLE_MEDITATION)),
            Scene {
                intense: true,
                ..enemy
            },
            Scores,
        ),
        (
            "battle meditation, intense, opening, valor running",
            offer(Choice::Power(BATTLE_MEDITATION)),
            Scene {
                intense: true,
                intense_buff: true,
                ..opening(1000)
            },
            Scores,
        ),
        (
            "valor, intense, already running",
            running(Choice::Power(VALOR)),
            Scene {
                intense: true,
                ..enemy
            },
            Zero,
        ),
        (
            "medpac, companion, ally at 0.1",
            offer(Choice::Item(MEDPAC)),
            Scene {
                own_health: 0.1,
                lowest_ally_health: 0.1,
                ..companion
            },
            Zero,
        ),
        (
            "stim, companion, intense, opening",
            offer(Choice::Item(STIM)),
            Scene {
                fight_ms: 1000,
                intense: true,
                ..companion
            },
            Zero,
        ),
        (
            "medpac, enemy, own health 0.2",
            offer(Choice::Item(MEDPAC)),
            Scene {
                own_health: 0.2,
                lowest_ally_health: 0.2,
                ..enemy
            },
            Scores,
        ),
    ];
    let threshold = rules::ai::choices::THRESHOLD;
    for (name, option, scene, expect) in &rows {
        let found = score(option, scene);
        let passes = match expect {
            Scores => found >= threshold,
            Zero => found == 0.0,
        };
        assert!(passes, "{name}: expected {expect:?}, scored {found}");
    }
    let heal = offer(Choice::Power(HEAL));
    assert!(
        score(&heal, &hurt(0.2)) > score(&heal, &hurt(0.45)),
        "a heal scores higher the lower the ally's health is"
    );
    assert_eq!(choose(&[], &enemy, 0.0), None, "no options");
    let tie = [offer(Choice::Power(LIGHTNING)), offer(Choice::Power(STUN))];
    let stun_or_heal = [offer(Choice::Power(STUN)), heal];
    let picks = [
        (
            "a tie, pick 0.0",
            &tie[..],
            enemy,
            0.0,
            Choice::Power(LIGHTNING),
        ),
        (
            "a tie, pick 0.999",
            &tie[..],
            enemy,
            0.999,
            Choice::Power(STUN),
        ),
        (
            "a heal before a stun, pick 0.0",
            &stun_or_heal[..],
            hurt(0.4),
            0.0,
            Choice::Power(HEAL),
        ),
        (
            "a heal before a stun, pick 0.999",
            &stun_or_heal[..],
            hurt(0.4),
            0.999,
            Choice::Power(HEAL),
        ),
    ];
    for (name, options, scene, pick, expected) in picks {
        assert_eq!(choose(options, &scene, pick), Some(expected), "{name}");
    }
    let kinds = [
        (
            "a listed row wins over the flag",
            STUN,
            Some(true),
            Some((Kind::Disable, false)),
        ),
        (
            "a listed droids-only row",
            DROID_STUN,
            Some(false),
            Some((Kind::Disable, true)),
        ),
        (
            "an unlisted hostile row",
            UNLISTED,
            Some(true),
            Some((Kind::Damage, false)),
        ),
        (
            "an unlisted friendly row",
            UNLISTED,
            Some(false),
            Some((Kind::Buff, false)),
        ),
        ("an unlisted row without facts", UNLISTED, None, None),
        // A row of the games' own that no kind lists is never a choice,
        // whatever its talent facts say.
        (
            "affect mind, a named row in no kind list, friendly",
            AFFECT_MIND,
            Some(false),
            None,
        ),
        (
            "affect mind, a named row in no kind list, hostile",
            AFFECT_MIND,
            Some(true),
            None,
        ),
        // The games' own rows that the engine does not name and the
        // rulebook never lets a character choose (`ai.choices.kinds.never`).
        ("force camouflage, friendly", CAMOUFLAGE, Some(false), None),
        (
            "battle precognition, friendly",
            BATTLE_PRECOGNITION,
            Some(false),
            None,
        ),
        (
            "breath control, friendly",
            BREATH_CONTROL,
            Some(false),
            None,
        ),
    ];
    for (name, row, hostile, expected) in kinds {
        assert_eq!(kind_of(row, hostile), expected, "{name}");
    }
}

/// The wait after a choice stays inside its tier's least and most for the
/// lowest and the highest `pick`.
#[test]
fn choice_waits() {
    use rules::ai::choices::wait_ms::{BOSS, ORDINARY, SKILLED};
    use sim::combat::pace::Tier;
    for (tier, [least, most]) in [
        (Tier::Ordinary, ORDINARY),
        (Tier::Skilled, SKILLED),
        (Tier::Boss, BOSS),
    ] {
        for pick in [0.0, 0.999] {
            let wait = sim::combat::choose::wait_ms(tier, pick);
            assert!(
                (least..=most).contains(&wait),
                "{tier:?} at pick {pick} waits {wait} ms, outside {least} to {most}"
            );
        }
    }
}

/// Whether a fight is intense for the party (`sim::combat::choose`), how
/// long it stays intense, and how many items of a kind a companion keeps.
/// An enemy finds an intense fight with the sides swapped, and casts an
/// intense-fight buff only then (staged cases added 2026-11-19, awaiting
/// approval).
#[test]
fn intense_fights() {
    use rules::ai::choices::{INTENSE_HOLD_MS, KEEP_ITEMS};
    use sim::combat::choose::{Member, held_until, intense, intense_now, may_spend};
    let at = |level: u8, health: f32, boss: bool| Member {
        level,
        health,
        boss,
    };
    let many = |count: usize, level: u8| vec![at(level, 1.0, false); count];
    let boss = |level: u8| vec![at(level, 1.0, true)];
    let hurt_party = |health: f32| vec![at(10, 1.0, false), at(10, health, false)];
    let cases: [(&str, Vec<Member>, Vec<Member>, bool); 7] = [
        ("weaker hostiles", many(3, 10), many(2, 5), false),
        ("equal strength", many(3, 10), many(3, 10), true),
        ("a boss counts double", many(3, 10), boss(15), true),
        ("four hostiles", many(3, 10), many(4, 1), true),
        ("a member below 40%", hurt_party(0.39), many(1, 1), true),
        ("a member at 40%", hurt_party(0.40), many(1, 1), false),
        ("no hostiles", hurt_party(0.1), Vec::new(), false),
    ];
    for (name, party, hostiles, expected) in &cases {
        assert_eq!(intense_now(party, hostiles), *expected, "{name}");
    }

    // A fight that goes calm and intense again within the hold stays
    // intense the whole time. It is calm exactly the hold after the last
    // intense look.
    let looks: [(u64, bool); 4] = [(0, true), (4_000, false), (8_000, true), (12_000, false)];
    let mut until = 0;
    for (now, intense_look) in looks {
        if intense_look {
            until = held_until(now);
        }
        assert!(intense(now, until), "calm at {now} ms");
    }
    let last = 8_000;
    assert!(intense(last + INTENSE_HOLD_MS - 1, until));
    assert!(!intense(last + INTENSE_HOLD_MS, until));
    assert!(!intense(0, 0), "a fresh mind is calm");

    for (left, expected) in [(KEEP_ITEMS, false), (KEEP_ITEMS + 1, true), (0, false)] {
        assert_eq!(may_spend(left), expected, "{left} left");
    }
    probed("mind snapshot", &sim::combat::ai::probes(), 1);
    // An enemy finds an intense fight with the sides swapped. A Dark Jedi
    // who knows Force Valor casts it alone against the player and two
    // companions, and not with three allies against the player alone.
    probed(
        "enemy intense fights",
        &sim::enemy_choice_probes::intense_probes(),
        2,
    );
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
/// as it stands, body and all; the creature played outliving the script
/// that destroys it, and its creature gone when it is taken off the table
/// (TSL's B-4D4; added and approved 2026-11-12; its companion number is no
/// longer available before the script makes the droid available, as on
/// Telos, amended and approved 2026-11-14 for issue 396); armour the player took
/// from a companion made from the table stays single when a script destroys
/// that companion and makes it again (added and approved 2026-11-13, issue
/// 366); a companion leading the party arrives in a module facing the
/// arrival's way (issue 412, approved 2026-11-16); the companion played
/// taken through two modules that lack its template, still played and with
/// its facts in both (TSL's B-4D4 through the docks, added 2026-11-17,
/// awaiting approval); the sim alone.
#[test]
fn switch_player_character() {
    probed("switch", &sim::switch::probes(), 27);
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
/// install. The same probes also read a `global.jrl` with one bad quest name
/// and check that every quest is still read.
#[test]
fn kill_xp_tables() {
    probed("kill xp tables", &session::journal::probes(), 4);
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

/// A swoop's sideways motion and hits as both executables have them
/// (`sim::minigame::swoop_probes`, docs/MINIGAMES.md): the drift after a
/// second of full and half steering, the walls (K1 keeps the drift, TSL
/// stops a fast one), a pad's reach counting the swoop's height, a buoy met
/// only by the swoop's middle; no install.
#[test]
fn minigame_swoop() {
    probed("minigame swoop", &sim::minigame::swoop_probes::probes(), 7);
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
/// class grants Master Toughness, twice alike; no install. The
/// `same_level_picks` check was added and approved on 2026-11-12. A level
/// with two feats and two powers takes Flurry and Improved Flurry, and
/// Force Push and Force Whirlwind, in either order. A level with Whirlwind
/// alone is refused, naming Push. The `ranks_apart_from_bonuses` check was
/// added and approved on 2026-11-12. A character with
/// Gear Head and a belt that adds Repair has a Repair total with both, and
/// still buys Repair ranks up to the level + 3, because the cap counts its
/// own ranks alone. The `power_level_total` check was added on 2026-11-12
/// and was approved on 2026-11-16. A power's level is held to the character's total
/// level, so a level 8 soldier taking a first Jedi level may pick a power
/// of level 9 but not one of level 15. The `double_weapon_balanced` check
/// was proposed on 2026-11-17. A double weapon counts as its own balanced
/// off-hand weapon, so Master Two-Weapon Fighting leaves 0 to hit on the
/// main end and -2 on the off end. A quarterstaff counts as a double
/// weapon, so an untrained character takes -4 and -10.
#[test]
fn feats() {
    probed("feats", &sim::feats::probes(), 20);
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
    probed("save edits", &edit::probes::all(), 28);
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

/// A playthrough checkpoint written as a player's save
/// (`saves::checkpoint_probes`, proposed 2026-11-18). A checkpoint made on
/// the proving ground with the player immortal and dealing ten times the
/// damage is written as a save. The save reads back whole with neither
/// flag on its player and nothing else changed, the world loaded from it
/// has neither flag, and the checkpoint's own bytes still carry both. No
/// install is needed.
#[test]
fn checkpoint_saves_without_test_aids() {
    probed("checkpoint saves", &saves::checkpoint_probes(), 3);
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

/// The co-op lobby on the wire (`net::session::lobby_probes`): each lobby
/// message read back, guests joining into the lobby, their requests, a
/// guest seeing its own slot change, Start before the welcome, a guest
/// leaving, the host closing the lobby, another build refused, a guest who
/// asked before the lobby opened, and a host without a lobby welcoming at
/// once.
#[test]
fn coop_lobby_wire() {
    probed(
        "lobby wire",
        &net_checks(net::session::lobby_probes::probes()),
        16,
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
/// packets with no panic. The check "handshake: a timeout says whether the
/// host answered" was added and approved on 2026-11-12 (issue 340). A join
/// no host answers times out with the host never heard. A join whose host
/// challenges and never accepts times out with the host heard.
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
/// scores starting at the class's recommended ones, as a mod's classes.2da
/// may set them, are taken (added and approved 2026-11-13, issue 370);
/// `sim::player_probes`, no install.
#[test]
fn player_creation() {
    probed("creation", &sim::player_probes::creation(), 13);
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

/// A body armour dresses its wearer by its base item's `bodyvar`
/// (`baseitems.2da`), as both games' executables set an item's body
/// variation when they load it. The UTI's own `BodyVariation` field is never
/// read. Mods fill that field with anything: Solomon's Revenge's Jedi robes
/// carry 50 to 64, and kjk drew them in the wearer's unarmoured body. With
/// a K1 install (else skipped): a man's scout build (row 137) in the Endar
/// Spire's clothes (`g_a_clothes01`, base item 85, `B`) whose field says 50
/// wears `pmbbm`, and in a Jedi robe (`g_a_jedirobe01`, base item 35, `I`)
/// whose field says 1 wears `pmbim` (proposed and approved
/// 2026-11-12).
#[test]
fn armour_body_from_base_item() {
    use asset_odyssey::module::Source;
    use fmt_odyssey::ResType;
    use fmt_odyssey::gff::{Gff, GffValue};

    /// The install with one UTI's `BodyVariation` changed.
    struct Changed<'s> {
        install: &'s dyn Source,
        item: &'s str,
        bytes: Vec<u8>,
    }
    impl Source for Changed<'_> {
        fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
            if restype == ResType::UTI && resref.eq_ignore_ascii_case(self.item) {
                return Some(self.bytes.clone());
            }
            self.install.read(resref, restype)
        }
        fn exists(&self, resref: &str, restype: ResType) -> bool {
            self.install.exists(resref, restype)
        }
    }

    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("K1's armour skipped: no KOTOR install (set KJK_KOTOR1 in .env)");
        return;
    }
    let install = module_source(&discovery, vfs::Game::Kotor1, "end_m01aa").expect("it opens");
    for (item, field, body) in [
        ("g_a_clothes01", 50, "pmbbm"),
        ("g_a_jedirobe01", 1, "pmbim"),
    ] {
        let bytes = install.read(item, ResType::UTI).expect("the item is there");
        let mut uti = Gff::parse(&bytes).expect("the item reads");
        let variation = (uti.root.fields.iter_mut())
            .find(|field| field.label == b"BodyVariation")
            .expect("the item has the field");
        variation.value = GffValue::Byte(field);
        let changed = Changed {
            install: &install,
            item,
            bytes: uti.write(),
        };
        let look = asset_odyssey::creature::resolve_appearance(
            &changed,
            137,
            Some(item.to_string()),
            &mut asset_odyssey::tables::Tables::default(),
        )
        .expect("the appearance resolves");
        assert_eq!(look.body, body, "{item} with BodyVariation {field}");
    }
}

/// KOTOR 2's armour dresses its wearer in the body of its base item's
/// letter, and KOTOR 2's executable reads letters up to Z. GitHub issue 368
/// reported Zeison Sha armour and Jedi robes drawn as the wearer's
/// underwear, because kjk read only letters A to J, as KOTOR 1's
/// executable does. With a TSL install (else skipped), a man's medium body
/// (row 137) wears these bodies. The flight suit (`a_light_11`, letter K)
/// wears `pmbkm`. Zeison Sha armour (`a_robe_06`, letter M) wears `pmbmm`.
/// A Jedi Knight robe (`a_robe_x01`) and a Jedi Master robe (`a_robe_05`),
/// both letter N, wear `pmbnm`. A Jedi robe (`a_robe_02`, letter I) still
/// wears `pmbim` (proposed and approved 2026-11-13, issue 368).
#[test]
fn tsl_armour_body_letters() {
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor2).is_none() {
        println!("TSL's armour skipped: no KOTOR 2 install (set KJK_KOTOR2 in .env)");
        return;
    }
    let install = module_source(&discovery, vfs::Game::Kotor2, "152har").expect("it opens");
    let mut tables = asset_odyssey::tables::Tables::default();
    for (item, body) in [
        ("a_light_11", "pmbkm"),
        ("a_robe_06", "pmbmm"),
        ("a_robe_x01", "pmbnm"),
        ("a_robe_05", "pmbnm"),
        ("a_robe_02", "pmbim"),
    ] {
        let look = asset_odyssey::creature::resolve_appearance(
            &install,
            137,
            Some(item.to_string()),
            &mut tables,
        )
        .expect("the appearance resolves");
        assert_eq!(look.body, body, "{item}");
    }
}

/// A cell of KOTOR 2's `appearance.2da` that reads `&` and a row number
/// draws that row instead, wearing the same armour. GitHub issue 584
/// reported the Handmaiden drawn in her own clothes with nothing on. Her
/// row (454) draws her clothes and sends every other letter to row 638,
/// which has a head. With a TSL install (else skipped), she wears these
/// bodies. With nothing on she wears `p_handmaidenba` from row 638. In the
/// basic clothes (`g_a_clothes01`, letter B) she wears her own clothes,
/// `p_handmaidenbb`. In a Jedi robe (`a_robe_02`, letter I) she wears
/// row 638's `pfbim`. In her mother's robe (`a_robe_x01`, letter N) her
/// row names her own clothes, so she wears `p_handmaidenbb` (proposed
/// 2026-11-19, issue 584).
#[test]
fn tsl_appearance_redirects() {
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor2).is_none() {
        println!("TSL's redirects skipped: no KOTOR 2 install (set KJK_KOTOR2 in .env)");
        return;
    }
    let install = module_source(&discovery, vfs::Game::Kotor2, "152har").expect("it opens");
    let mut tables = asset_odyssey::tables::Tables::default();
    for (item, appearance, body) in [
        (None, 638, "p_handmaidenba"),
        (Some("g_a_clothes01"), 454, "p_handmaidenbb"),
        (Some("a_robe_02"), 638, "pfbim"),
        (Some("a_robe_x01"), 454, "p_handmaidenbb"),
    ] {
        let look = asset_odyssey::creature::resolve_appearance(
            &install,
            454,
            item.map(str::to_string),
            &mut tables,
        )
        .expect("the appearance resolves");
        assert_eq!(
            (look.appearance, look.body.as_str()),
            (appearance, body),
            "{item:?}"
        );
    }
}

/// A template's off hand weapon is drawn only when the equip screen would
/// let it be held there. GitHub issue 569 reported a Mandalorian in the
/// Sandral Grounds on Dantooine (`dan14ad_mand02`) with a blaster rifle in
/// each hand: his template lists one in each, and a blaster rifle can be
/// equipped only in the main hand. With a K1 install (else skipped), he
/// holds the rifle in his main hand and nothing in his off hand (proposed
/// 2026-11-19, issue 569).
#[test]
fn off_hand_rifle_not_held() {
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("K1's Mandalorian skipped: no KOTOR install (set KJK_KOTOR1 in .env)");
        return;
    }
    let source = module_source(&discovery, vfs::Game::Kotor1, "danm14ad").expect("it opens");
    let look = asset_odyssey::creature::resolve(&source, "dan14ad_mand02").expect("he resolves");
    assert!(look.right_weapon.is_some(), "a rifle in his main hand");
    assert_eq!(look.left_weapon, None, "nothing in his off hand");
}

/// A model's reference node carries the model it names, as the games draw
/// one (`fmt_odyssey::mdl::Model::attach_references`): Lightsaber Hilt
/// Variety's hilts are a blade and a `ref_hilt` node naming the hilt's
/// model, and kjk drew the blade alone. The named model's tree hangs under
/// the node and moves with it. A reference naming `NULL` is left empty, and
/// one naming a model that is not there is reported. No installs (proposed
/// and approved 2026-11-12).
#[test]
fn reference_nodes_carry_their_models() {
    use asset_odyssey::model::ModelIr;
    use fmt_odyssey::mdl::{Face, Mesh, Model, Node, Reference};

    let node = |name: &str, position: [f32; 3], reference: Option<&str>| Node {
        flags: 0,
        number: 0,
        name: name.into(),
        position,
        orientation: [1.0, 0.0, 0.0, 0.0],
        controllers: Vec::new(),
        light: None,
        reference: reference.map(|model| Reference {
            model: model.into(),
            reattachable: false,
        }),
        emitter: None,
        mesh: None,
        children: Vec::new(),
        animated: false,
    };
    let model = |name: &str, root: Node| Model {
        name: name.into(),
        classification: 0,
        subclassification: 0,
        affected_by_fog: false,
        supermodel: None,
        bounding_box: [0.0; 6],
        radius: 0.0,
        animation_scale: 1.0,
        root,
        animations: Vec::new(),
    };
    // The hilt: one triangle, 0.1 above its root.
    let mut grip = node("grip", [0.0, 0.0, 0.1], None);
    grip.mesh = Some(Mesh {
        bounds: [0.0; 6],
        textures: ["w_lghtsbr_001".into(), "".into(), "".into(), "".into()],
        diffuse: [1.0; 3],
        ambient: [1.0; 3],
        transparency_hint: 0,
        render: true,
        shadow: false,
        lightmapped: false,
        background: false,
        beaming: false,
        rotate_texture: false,
        animate_uv: None,
        positions: vec![[0.0; 3], [0.1, 0.0, 0.0], [0.0, 0.1, 0.0]],
        normals: Vec::new(),
        uv1: Vec::new(),
        uv2: Vec::new(),
        faces: vec![Face {
            normal: [0.0, 0.0, 1.0],
            distance: 0.0,
            material: 0,
            adjacent: [0xffff; 3],
            vertices: [0, 1, 2],
        }],
        indices: Vec::new(),
        skin: None,
        dangly: None,
        aabb: None,
        saber: false,
    });
    let mut hilt_root = node("lghtsbr_blue", [0.0; 3], None);
    hilt_root.children.push(grip);
    let hilt = model("lghtsbr_blue", hilt_root);

    // The saber: `dummy01` 0.2 up, holding the reference to the hilt.
    let mut dummy = node("dummy01", [0.0, 0.0, 0.2], None);
    for (name, named) in [
        ("ref_hilt", "lghtsbr_blue"),
        ("ref_none", "NULL"),
        ("ref_gone", "lghtsbr_gone"),
    ] {
        dummy.children.push(node(name, [0.0; 3], Some(named)));
    }
    let mut root = node("w_lghtsbr_001", [0.0; 3], None);
    root.children.push(dummy);
    let mut saber = model("w_lghtsbr_001", root);

    let mut asked = Vec::new();
    let missing = saber.attach_references(&mut |name| {
        asked.push(name.to_string());
        name.eq_ignore_ascii_case("lghtsbr_blue")
            .then(|| hilt.clone())
    });
    assert_eq!(
        asked,
        ["lghtsbr_blue", "lghtsbr_gone"],
        "NULL is not asked for"
    );
    assert_eq!(missing, ["lghtsbr_gone"]);
    let ir = ModelIr::from_model(&saber);
    let grip = ir
        .node("grip")
        .expect("the hilt hangs under its reference node");
    let parent = grip.parent.map(|parent| ir.nodes[parent].name.as_str());
    assert_eq!(parent, Some("lghtsbr_blue"));
    let height = grip.world[2][3];
    assert!(
        (height - 0.3).abs() < 1e-6,
        "the grip is 0.2 + 0.1 up, not {height}"
    );
    let drawn = ir
        .meshes
        .iter()
        .filter(|mesh| ir.nodes[mesh.node].name == "grip");
    assert_eq!(drawn.count(), 1, "the hilt's mesh is drawn");
}

/// A model node whose flag word is none of the twelve the games know is
/// read as a plain node (QUIRKS Q-0565). GitHub issue 451 reported that
/// the door to M4-78's Central Zone loaded nothing. kjk refused the mod's
/// `l_drdprot` model for a node with the word 0x6310, and with the model
/// the area. A made-up model has four nodes under its root. The first has
/// the word 0x6310, and its lists of children and controllers point
/// outside the file. It keeps its name and its place, and it has no
/// children, no controllers and no mesh. The second is a trigger (0x0401),
/// and its child is read. The third has the mesh bit without the header
/// bit (0x0020) and a name number outside the name table. It is read
/// without a mesh and without a name. The fourth, a plain node, is read
/// after them. No installs (proposed and approved 2026-11-15).
#[test]
fn unknown_model_nodes_read_plain() {
    use fmt_odyssey::mdl::Model;

    const NODE: usize = 80;
    const NAMES: [&str; 5] = ["probe", "odd", "trigger", "below", "last"];
    let outside = [0xffff_fff0u32, 7, 7];
    // (flag word, name number, lists of children, keys and key data).
    // Each node sits `NODE` bytes after the one before it.
    let names_at = 196;
    let nodes_at = names_at + NAMES.len() * 4 + NAMES.iter().map(|n| n.len() + 1).sum::<usize>();
    let at = |i: usize| (nodes_at + i * NODE) as u32;
    let none = [0u32; 3];
    let nodes: [(u16, u16, [u32; 3], [u32; 3]); 6] = [
        (0x0001, 0, [at(6), 4, 4], none),
        (0x6310, 1, outside, outside),
        (0x0401, 2, [at(6) + 16, 1, 1], none),
        (0x0001, 3, none, none),
        (0x0001, 4, none, none),
        (0x0020, 99, none, none),
    ];
    let mut data = vec![0u8; names_at];
    let put = |data: &mut Vec<u8>, at: usize, words: &[u32]| {
        for (i, word) in words.iter().enumerate() {
            data[at + i * 4..at + i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
    };
    data[8..13].copy_from_slice(b"probe");
    put(&mut data, 40, &[at(0)]);
    data[136..140].copy_from_slice(b"null");
    put(&mut data, 168, &[at(0)]);
    put(
        &mut data,
        184,
        &[names_at as u32, NAMES.len() as u32, NAMES.len() as u32],
    );
    let mut name_at = names_at + NAMES.len() * 4;
    for name in NAMES {
        data.extend((name_at as u32).to_le_bytes());
        name_at += name.len() + 1;
    }
    for name in NAMES {
        data.extend(name.bytes().chain([0]));
    }
    for (i, (flags, name, children, keys)) in nodes.into_iter().enumerate() {
        let start = data.len();
        data.resize(start + NODE, 0);
        data[start..start + 2].copy_from_slice(&flags.to_le_bytes());
        data[start + 4..start + 6].copy_from_slice(&name.to_le_bytes());
        put(&mut data, start + 16, &[(i as f32).to_bits()]);
        put(&mut data, start + 28, &[1.0f32.to_bits()]);
        put(&mut data, start + 44, &children);
        put(&mut data, start + 56, &keys);
        put(&mut data, start + 68, &keys);
    }
    // The root's children (the odd node, the trigger, the node with the
    // mesh bit, the last node), then the trigger's child.
    for child in [at(1), at(2), at(5), at(4), at(3)] {
        data.extend(child.to_le_bytes());
    }
    let mut mdl = vec![0u8; 12];
    mdl[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
    mdl.extend(data);

    let model = Model::parse(&mdl, &[]).expect("the model reads");
    let read: Vec<_> = (model.root.children.iter())
        .map(|node| {
            (
                node.name.as_str(),
                node.of_known_type(),
                node.children.len(),
                node.controllers.len(),
                node.mesh.is_some(),
                node.position[0],
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("odd", false, 0, 0, false, 1.0),
            ("trigger", true, 1, 0, false, 2.0),
            ("", false, 0, 0, false, 5.0),
            ("last", true, 0, 0, false, 4.0),
        ],
        "each node's name, known type, children, controllers, mesh and place"
    );
    assert_eq!(model.root.children[1].children[0].name, "below");
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
    // A pre-release's save whose snapshot version `main` later gave to
    // another change (#354): read by the pre-release's own layout, named
    // by its file, loads as the fixtures above do.
    let mut prereleases = 0;
    for entry in std::fs::read_dir(dir.join("prerelease")).expect("the pre-release fixtures") {
        let path = entry.expect("a pre-release fixture").path();
        let name = path
            .file_stem()
            .expect("a fixture's name")
            .to_string_lossy();
        let bytes = std::fs::read(&path).expect("a pre-release fixture reads");
        let (save, found) = sim::Save::decode_versioned(&bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(found.prerelease, Some(&*name), "{}", path.display());
        // Movie Duels' pre-releases saved under its saber rules.
        if name.contains("-md-") {
            assert_eq!(
                save.snapshot.saber_rules,
                sim::combat::saber::SaberRules::DuelAdvanced,
                "{}",
                path.display()
            );
        }
        let again = sim::Save::decode(&save.encode()).expect("a re-encoded save decodes");
        assert_eq!(again, save, "{}: re-encoding changes it", path.display());
        save.world(&level, 2, None)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        prereleases += 1;
    }
    assert!(prereleases > 0, "no pre-release fixtures");
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

/// The loader of Movie Duels' stance sets on made-up skeletons
/// (`kjk_game::stance_sets::probes`). Needs `--features rig`; no installs.
#[test]
fn stance_sets_table() {
    #[cfg(not(feature = "rig"))]
    println!("stance_sets_table skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("stance sets table", &kjk_game::stance_sets::probes(), 12);
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

/// The finder of `kjk original`'s library (`original_find`) on the installed
/// games' own programs. For each installed KOTOR 1 and KOTOR 2, the program
/// is read from its file and the finder must find every function and field.
/// No address of a game is compared, since none may be written in kjk. The
/// two functions it finds must lie in the program's code, the app manager's
/// global must lie outside it, and the client app's internal half must lie
/// one pointer after the client app's start. A game that is not installed is
/// skipped, and so is a Windows program that Steam's DRM still holds
/// encrypted. (proposed 2026-11-16)
#[test]
fn original_warp_list_found() {
    let discovery = discovery();
    let mut checked = 0;
    for (game, mac_program, windows_program) in [
        (vfs::Game::Kotor1, "KOTOR_Exe", "swkotor.exe"),
        (vfs::Game::Kotor2, "KOTOR2sub", "swkotor2.exe"),
    ] {
        let Some(install) = discovery.get(game) else {
            println!(
                "{}: skipped, not found (set {} in .env)",
                game.key(),
                game.env_var()
            );
            continue;
        };
        let Some(path) = program_file(&install.root, mac_program, windows_program) else {
            println!(
                "{}: skipped, no {mac_program} or {windows_program}",
                game.key()
            );
            continue;
        };
        let bytes = std::fs::read(&path).expect("the program reads");
        let (image, wrapped) =
            program_image(&bytes).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let found = match original_find::find(&image) {
            Ok(found) => found,
            Err(_) if wrapped => {
                println!(
                    "{}: skipped, its code is encrypted by Steam's DRM",
                    game.key()
                );
                continue;
            }
            Err(missing) => panic!("{}: {missing}", path.display()),
        };
        let code = image.code_at..image.code_at + image.code.len() as u64;
        assert!(code.contains(&found.on_module_picked), "{}", game.key());
        assert!(code.contains(&found.populate_modules), "{}", game.key());
        assert_ne!(found.on_module_picked, found.populate_modules);
        assert!(!code.contains(&found.app_manager), "{}", game.key());
        assert!(found.menu_path[0] > 0, "{}", game.key());
        assert_eq!(found.menu_path[1], image.bitness.pointer_size());
        for (name, offset) in [
            ("menu", found.menu_path[2]),
            ("drawn", found.drawn),
            ("picked", found.picked),
            ("modules", found.modules),
            ("control_check", found.control_check),
        ] {
            assert!(offset > 0, "{}: {name} is at offset 0", game.key());
        }
        assert_ne!(found.picked, found.modules);
        println!("{}: {} found", game.key(), path.display());
        checked += 1;
    }
    println!("{checked} programs checked");
}

/// The program of an install: the Mac port's, inside its app, or the
/// Windows build's, in the install's folder.
fn program_file(
    root: &std::path::Path,
    mac_program: &str,
    windows_program: &str,
) -> Option<std::path::PathBuf> {
    let windows = root.join(windows_program);
    if windows.is_file() {
        return Some(windows);
    }
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(|app| app.join("Contents/MacOS").join(mac_program))
        .find(|program| program.is_file())
}

/// A program file as the finder reads it, and whether it is wrapped in
/// Steam's DRM. A Mac program is a 64-bit Mach-O file, or a universal file
/// that holds one. A Windows program is a 32-bit PE file.
fn program_image(bytes: &[u8]) -> Result<(original_find::Image<'_>, bool), String> {
    use original_find::{Bitness, Image};
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    match bytes.get(..4) {
        Some([0xca, 0xfe, 0xba, 0xbe]) => {
            // A universal file's header is big-endian. Each of its programs
            // has 20 bytes: its CPU type, CPU subtype, offset, size and
            // alignment.
            let be = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
            const X86_64: u32 = 0x0100_0007;
            let offset = (0..be(4) as usize)
                .map(|index| 8 + 20 * index)
                .find(|entry| be(*entry) == X86_64)
                .map(|entry| be(entry + 8) as usize)
                .ok_or("the universal file holds no x86_64 program")?;
            program_image(&bytes[offset..])
        }
        Some([0xcf, 0xfa, 0xed, 0xfe]) => {
            let section = |segment: &[u8], name: &[u8]| {
                let mut at = 32;
                for _ in 0..u32_at(16) {
                    const SEGMENT_64: u32 = 0x19;
                    if u32_at(at) == SEGMENT_64 {
                        for index in 0..u32_at(at + 64) as usize {
                            let header = at + 72 + 80 * index;
                            let text = |from: usize| {
                                let field = &bytes[header + from..header + from + 16];
                                &field[..field.iter().position(|b| *b == 0).unwrap_or(16)]
                            };
                            if text(0) == name && text(16) == segment {
                                let address = u64::from_le_bytes(
                                    bytes[header + 32..header + 40].try_into().expect("8 bytes"),
                                );
                                let size = u64::from_le_bytes(
                                    bytes[header + 40..header + 48].try_into().expect("8 bytes"),
                                ) as usize;
                                let offset = u32_at(header + 48) as usize;
                                return Some((&bytes[offset..offset + size], address));
                            }
                        }
                    }
                    at += u32_at(at + 4) as usize;
                }
                None
            };
            let (code, code_at) = section(b"__TEXT", b"__text").ok_or("no __text section")?;
            let strings = section(b"__TEXT", b"__cstring").ok_or("no __cstring section")?;
            let image = Image {
                code,
                code_at,
                data: vec![strings],
                bitness: Bitness::Bits64,
            };
            Ok((image, false))
        }
        Some([b'M', b'Z', ..]) => {
            let header = u32_at(0x3c) as usize;
            let optional = u16_at(header + 20) as usize;
            let base = u64::from(u32_at(header + 24 + 28));
            let mut code = None;
            let mut data = Vec::new();
            let mut wrapped = false;
            for index in 0..u16_at(header + 6) as usize {
                let entry = header + 24 + optional + 40 * index;
                let name = &bytes[entry..entry + 8];
                let size = (u32_at(entry + 8) as usize).min(u32_at(entry + 16) as usize);
                let address = base + u64::from(u32_at(entry + 12));
                let raw = u32_at(entry + 20) as usize;
                let section = (&bytes[raw..raw + size], address);
                match name {
                    // Steam's DRM adds this section. It decrypts the code
                    // only once the game runs.
                    b".bind\0\0\0" => wrapped = true,
                    b".text\0\0\0" => code = Some(section),
                    b".rdata\0\0" | b".data\0\0\0" => data.push(section),
                    _ => {}
                }
            }
            let (code, code_at) = code.ok_or("no .text section")?;
            let image = Image {
                code,
                code_at,
                data,
                bitness: Bitness::Bits32,
            };
            Ok((image, wrapped))
        }
        _ => Err("not a Mach-O or PE program".into()),
    }
}

const PREPASS_READERS: [(&str, &str); 3] = [
    (
        "upscale.rs",
        "kjk's upscaler reads the depth and the motion. `temporal::apply` declares `Need::TEMPORAL` for it.",
    ),
    (
        "odyssey.wgsl",
        "Ambient occlusion reads the depth, and its debug view of the normals reads the normals. `ambient_occlusion::mark` declares `Need::AMBIENT_OCCLUSION` for both.",
    ),
    (
        "motion_debug.wgsl",
        "The motion debug view reads the motion. `temporal::motion_debug` declares `Need::TEMPORAL` for it.",
    ),
];

const PREPASS_READS: [&str; 5] = [
    "depth_prepass_texture",
    "normal_prepass_texture",
    "motion_vector_prepass_texture",
    "prepass_utils::",
    "ViewPrepassTextures",
];

/// Proposed 2026-11-18, awaiting approval.
#[test]
fn prepass_readers_declared() {
    fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("the game's sources") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(&path, files);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "rs" || ext == "wgsl")
            {
                files.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../game/src");
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut readers: Vec<String> = files
        .iter()
        .filter(|path| {
            let text = std::fs::read_to_string(path).expect("a source file");
            text.lines()
                .map(|line| line.split("//").next().unwrap_or(""))
                .any(|code| PREPASS_READS.iter().any(|read| code.contains(read)))
        })
        .map(|path| {
            let relative = path.strip_prefix(&root).expect("under the sources");
            relative.to_string_lossy().replace('\\', "/")
        })
        .collect();
    readers.sort();
    let mut listed: Vec<String> = PREPASS_READERS
        .iter()
        .map(|(file, _)| file.to_string())
        .collect();
    listed.sort();
    assert!(files.len() > 100, "the game's sources found at {root:?}");
    assert_eq!(
        readers, listed,
        "the files reading a prepass's texture, against PREPASS_READERS"
    );
    println!("{} prepass readers, each declared", readers.len());
}
