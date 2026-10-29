//! The I10 skeleton probes (`kjk rig <probe>`): the same JA saber combo on
//! a KOTOR character three ways.
//!
//! - `a`: the KOTOR body and head, JA's clips retargeted onto KOTOR's
//!   skeleton (`asset_ja::rig::retarget`).
//! - `b`: JA's skeleton wearing the KOTOR body and head, skinned onto it at
//!   JA's proportions (`asset_ja::rig::rebind`), playing JA's clips as they
//!   are.
//! - `c`: as `a`, after a KOTOR clip (its own idle), for the hybrid where
//!   KOTOR clips stay for everything but combat.
//!
//! All three can hold the same JA hilt, so only the skeleton differs;
//! `a` and `c` hold a KOTOR hilt by default (`--hilt`), with the JA blade
//! drawn from it.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::anim::{AnimationIr, NodeTrack};
use asset_odyssey::hooks;
use asset_odyssey::model::{Matrix, ModelIr};
use bevy::math::Vec3;
use vfs::{Discovery, Game};

use crate::force_fx;
use crate::rig_timeline;
use crate::saber_flight;
use crate::viewer::{self, BladeSpec, LoadedScene, Placed, Shown};
use crate::viewer_ja::{self, Files};
use crate::weapons::kotor_blades;

/// The combo every probe plays: medium style from the ready stance, top-left
/// to bottom-right, round to top-right, top-right to bottom-left, and back
/// to ready. The chain follows JA's saber move table (start, attack,
/// transition, attack, return; style 2 uses the `*2` animations).
pub const COMBO: &str =
    "BOTH_STAND2,BOTH_S2_S1_TL,BOTH_A2_TL_BR,BOTH_T2_BR_TR,BOTH_A2_TR_BL,BOTH_R2_BL_S1,BOTH_STAND2";

/// The JA body whose `*r_hand` bolt holds the hilt (every `_humanoid`
/// model shares the skeleton).
const JA_BODY: &str = "models/players/kyle/model.glm";

pub struct Options<'a> {
    pub probe: &'a str,
    pub body: &'a str,
    pub head: &'a str,
    pub moves: &'a str,
    pub saber: &'a str,
    /// The KOTOR clip probe `c` plays first.
    pub kotor_clip: &'a str,
    /// Where to write the measured nodes' positions (CSV), if anywhere.
    pub trace: Option<&'a std::path::Path>,
    /// For `a` and `c`: KOTOR hilts instead of the JA ones.
    pub kotor_hilt: bool,
    /// The KOTOR blades' colours, per hand (the last one for the rest).
    pub colors: Vec<&'a str>,
    /// Reproduce the jolting reference quirks: the look's
    /// (`asset_ja::angles`), the player's (`asset_ja::g2::Player::quirks`),
    /// deaths' (`asset_ja::blend_ms`) and the thrown saber's
    /// (`saber_flight::fly`).
    pub quirks: bool,
}

/// A saber in a hand: which hand (JA bolt, KOTOR weapon node), the JA
/// saber, and the KOTOR hilt that stands in for it.
struct Hand {
    bolt: &'static str,
    weapon: &'static str,
    ja_saber: String,
    kotor_hilt: String,
}

/// A KOTOR hilt's model number for a blade colour: K1 makes one model per
/// colour (the blade planes' texture: `w_lsabreblue01`, `…red01`, …), the
/// same numbers for the single and the double-bladed hilts.
fn color_number(color: &str) -> Result<u32, String> {
    let colors = ["blue", "red", "green", "yellow", "purple"];
    colors
        .iter()
        .position(|c| c.eq_ignore_ascii_case(color))
        .map(|i| i as u32 + 1)
        .ok_or(format!("{color}: not a colour ({})", colors.join(", ")))
}

/// The hands a style fills, the `n`th hand's KOTOR blade in `colors[n]`
/// (or the last colour given).
fn hands(style: rig_timeline::Style, single: &str, colors: &[&str]) -> Result<Vec<Hand>, String> {
    use rig_timeline::Style;
    let color = |n: usize| {
        let color = colors.get(n).or(colors.last()).copied().unwrap_or("blue");
        color_number(color)
    };
    let hilt = |model: &str, n: usize| -> Result<String, String> {
        Ok(format!("{model}_{:03}", color(n)?))
    };
    let right = |ja_saber: &str, kotor_hilt| Hand {
        bolt: "*r_hand",
        weapon: hooks::RIGHT_HAND,
        ja_saber: ja_saber.to_string(),
        kotor_hilt,
    };
    Ok(match style {
        Style::Single => vec![right(single, hilt("w_lghtsbr", 0)?)],
        Style::Staff => vec![right("dual_1", hilt("w_dblsbr", 0)?)],
        Style::Dual => vec![
            right(single, hilt("w_lghtsbr", 0)?),
            Hand {
                bolt: "*l_hand",
                weapon: hooks::LEFT_HAND,
                ja_saber: single.to_string(),
                kotor_hilt: hilt("w_lghtsbr", 1)?,
            },
        ],
        Style::Melee => Vec::new(),
    })
}

/// The KOTOR clip whose first frame gives the fingers' grip: the ready
/// stance of KOTOR's matching weapon group (a single sword 2, a double-
/// bladed sword 3, two swords 4, unarmed 8: how reone
/// numbers them).
fn hand_pose(style: rig_timeline::Style) -> &'static str {
    use rig_timeline::Style;
    match style {
        Style::Single => "g2r1",
        Style::Staff => "g3r1",
        Style::Dual => "g4r1",
        Style::Melee => "g8r1",
    }
}

pub fn load(discovery: &Discovery, options: &Options) -> Result<LoadedScene, String> {
    // A thrown saber's catch cues the torso, so the timeline guesses it
    // until the flight has been flown on the retargeted hand, and is built
    // again with the catches flown (the motion before them is the same).
    let (scene, assumed, caught) = load_with(discovery, options, &[])?;
    if caught.len() == assumed.len() && caught.iter().zip(&assumed).all(|(a, b)| a == b) {
        return Ok(scene);
    }
    Ok(load_with(discovery, options, &caught)?.0)
}

/// The scene, with the catches its timeline assumed and those flown (ms).
fn load_with(
    discovery: &Discovery,
    options: &Options,
    catches: &[f32],
) -> Result<(LoadedScene, Vec<f32>, Vec<f32>), String> {
    let resolver = viewer::resolver(discovery, Game::Kotor1, None)?;
    let stack = viewer_ja::stack(discovery)?;
    let files = Files::new(&stack);
    let (glm, gla, gla_path) = viewer_ja::glm_and_gla(&files, JA_BODY)?;
    let mut notes = Vec::new();
    let mut cache = BTreeMap::new();
    let body = viewer::cached(&mut cache, &resolver, options.body)?
        .ok_or(format!("{}.mdl: not found", options.body))?;
    let head = viewer::cached(&mut cache, &resolver, options.head)?
        .ok_or(format!("{}.mdl: not found", options.head))?;
    let hook_node = body
        .node_index(hooks::HEAD_HOOK)
        .ok_or("headhook: not in the body")?;
    let hook = body.nodes[hook_node].world;
    // The JA clip, and the ground it covers.
    let rig_timeline::Timeline {
        clip: combo,
        angles,
        powers,
        ground,
        saber,
        moves,
        style,
        throws,
        catches: assumed,
    } = if rig_timeline::TIMELINES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(options.moves))
    {
        let cfg = viewer_ja::animation_cfg(&files, &gla_path)?;
        let name = options.moves.to_ascii_lowercase();
        let setup = rig_timeline::Setup {
            catches,
            quirks: options.quirks,
        };
        rig_timeline::timeline(&name, &gla, &cfg, &body, &setup, &mut notes)?
    } else {
        let (clip, moves) =
            viewer_ja::sequences_cued(&files, &gla, &gla_path, options.moves, options.quirks)?;
        rig_timeline::Timeline {
            clip,
            angles: Vec::new(),
            powers: Vec::new(),
            ground: crate::rig_ground::Ground::default(),
            saber: Vec::new(),
            // Named sequences of the pair's (style 6) or the staff's (7)
            // hold their sabers.
            style: if options.moves.contains("6_") || options.moves.contains("DUAL") {
                rig_timeline::Style::Dual
            } else if options.moves.contains("7_") || options.moves.contains("STAFF") {
                rig_timeline::Style::Staff
            } else {
                rig_timeline::Style::Single
            },
            moves,
            throws: Vec::new(),
            catches: Vec::new(),
        }
    };
    // Each hand's JA hilt and blades, its bolt on the JA body, and (for
    // `a`/`c`) the KOTOR hilt standing in.
    struct Held {
        hand: Hand,
        bolt: asset_ja::Bolt,
        ja_hilt: ModelIr,
        ja_blades: Vec<BladeSpec>,
        kotor: Option<(ModelIr, Vec<BladeSpec>)>,
    }
    let mut held = Vec::new();
    for hand in hands(style, options.saber, &options.colors)? {
        let bolt = asset_ja::bolt(&glm, hand.bolt).ok_or(format!("{}: no such tag", hand.bolt))?;
        if bolt.bone >= gla.bones.len() {
            return Err(format!(
                "{}: on bone {}, past the GLA's {} bones",
                hand.bolt,
                bolt.bone,
                gla.bones.len()
            ));
        }
        let (ja_hilt, ja_blades, note) = viewer_ja::hilt(&files, &hand.ja_saber)?;
        notes.push(note);
        let kotor = if options.kotor_hilt && options.probe != "b" {
            let ir = viewer::cached(&mut cache, &resolver, &hand.kotor_hilt)?
                .ok_or(format!("{}.mdl: not found", hand.kotor_hilt))?;
            Some(kotor_blades(ir).ok_or(format!("{}: no blade planes", hand.kotor_hilt))?)
        } else {
            None
        };
        held.push(Held {
            hand,
            bolt,
            ja_hilt,
            ja_blades,
            kotor,
        });
    }
    // Where the JA clip starts in the model's clip (after KOTOR's, in `c`).
    let mut ja_start = 0.0;
    let ja_length = combo.length;
    let mut period = ja_length;
    for (ja, kotor, degrees) in asset_ja::rig::mapping(&body, &gla) {
        notes.push(format!("map {ja} -> {kotor} (aligned {degrees:.1} deg)"));
    }

    let mut models = Vec::new();
    // Per hilt placed: (model index, blades, JA-textured, KOTOR weapon node
    // and the hilt's transform under it for tracing the trail).
    type Hilt = (usize, Vec<BladeSpec>, bool, Option<(usize, Option<Matrix>)>);
    let mut hilts: Vec<Hilt> = Vec::new();
    let mut traced: Option<AnimationIr> = None;
    match options.probe {
        "a" | "c" => {
            // The hands hold the hilts KOTOR's way, fingers as in KOTOR's
            // lightsaber ready stance.
            let grips: Vec<asset_ja::rig::Grip> = held
                .iter()
                .map(|h| {
                    let dir = h.ja_blades.first().map_or([0.0, 0.0, 1.0], |b| b.dir);
                    asset_ja::rig::Grip::new(&body, &gla, &h.bolt, dir, h.hand.weapon)
                        .map_err(|error| format!("{}: {error}", h.hand.bolt))
                })
                .collect::<Result<_, _>>()?;
            let pose_name = hand_pose(style);
            let stance = viewer::clip(&mut cache, &resolver, Game::Kotor1, &body, pose_name)?
                .ok_or(format!("{pose_name}: in no model of the chain"))?;
            let hold: Vec<(String, [f32; 4])> = stance
                .tracks
                .iter()
                .filter(|t| is_finger(&t.node))
                .filter_map(|t| {
                    let q = asset_odyssey::anim::sample_orientation(&t.orientations, 0.0)?;
                    Some((t.node.clone(), q))
                })
                .collect();
            notes.push(format!(
                "hand pose from {pose_name}: {} finger nodes",
                hold.len()
            ));
            let offset = |t: f32| ground.offset(t);
            let retargeted = asset_ja::rig::retarget_with(
                &body,
                &gla,
                &combo,
                &asset_ja::rig::Options {
                    ground: Some(&offset),
                    grips: grips.iter().collect(),
                    hold,
                    ja_sole: sole_height(&glm),
                },
            );
            let (clip, ja_from) = if options.probe == "c" {
                // Several comma-separated clips play whole, one after another,
                // before the JA clip; a single one plays its first seconds.
                let names: Vec<&str> = options.kotor_clip.split(',').map(str::trim).collect();
                let mut kotors = Vec::new();
                for name in &names {
                    let found = viewer::clip(&mut cache, &resolver, Game::Kotor1, &body, name)?
                        .ok_or(format!("{name}: in no model of the chain"))?;
                    notes.push(format!("kotor clip {name}: {:.2} s", found.length));
                    kotors.push(found);
                }
                let kotor = &kotors[0];
                // KOTOR's idle, the JA clip, KOTOR's idle again where the
                // JA clip left the body.
                let before = match kotors.as_slice() {
                    [one] => rig_timeline::slice(one, KOTOR_SECONDS.min(one.length)),
                    [first, rest @ ..] => rest.iter().fold((**first).clone(), |joined, next| {
                        then(&body, &joined, next, BLEND)
                    }),
                    [] => unreachable!("split yields at least one name"),
                };
                let root = body
                    .node_index(hooks::PELVIS)
                    .and_then(|pelvis| body.nodes[pelvis].parent)
                    .map_or(hooks::ROOT_DUMMY.to_string(), |r| {
                        body.nodes[r].name.clone()
                    });
                let travelled =
                    ground.offset(combo.length) / body.animation_scale.max(f32::EPSILON);
                let settle = rig_timeline::slice(kotor, KOTOR_SECONDS.min(kotor.length));
                let after = rig_timeline::shift(&settle, &root, travelled);
                let ja_from = before.length + BLEND;
                notes.push(format!(
                    "kotor clip {} for {:.2} s, then JA from {ja_from:.2} s to {:.2} s, then {} again",
                    kotor.name,
                    before.length,
                    ja_from + combo.length,
                    kotor.name
                ));
                let joined = then(&body, &before, &retargeted, BLEND);
                let clip = then(&body, &joined, &after, BLEND);
                period = clip.length;
                (clip, ja_from)
            } else {
                (retargeted, 0.0)
            };
            ja_start = ja_from;
            // Beside the trace, when each torso animation starts (s, on its
            // clock), to name what a trace's sample plays.
            if let Some(path) = options.trace {
                let rows: String = moves
                    .iter()
                    .map(|(ms, name)| format!("{:.4},{name}\n", ja_from + ms / 1000.0))
                    .collect();
                let moves_path = path.with_extension("moves.csv");
                if let Err(error) = std::fs::write(&moves_path, format!("t,move\n{rows}")) {
                    notes.push(format!("{}: {error}", moves_path.display()));
                }
            }
            notes.extend(rig_timeline::measure(
                &body,
                &clip,
                ja_from,
                ja_from + combo.length,
                options.trace,
            ));
            traced = Some(clip.clone());
            let clip = Some(Arc::new(clip));
            models.push(Placed {
                ir: body,
                offset: [0.0; 3],
                facing: 0.0,
                is_static: false,
                attach: None,
                hook: None,
                hook_local: None,
                clip: clip.clone(),
            });
            models.push(Placed {
                ir: head,
                offset: [0.0; 3],
                facing: 0.0,
                is_static: false,
                attach: Some(hook),
                hook: Some((0, hook_node)),
                hook_local: None,
                clip,
            });
            // A KOTOR hilt hangs at its weapon node as K1 hangs it; a JA
            // one turned to match (the hand's turn is the same either way).
            for (h, grip) in held.iter().zip(&grips) {
                let (ir, blades, local, ja) = match &h.kotor {
                    Some((ir, blades)) => (ir.clone(), blades.clone(), None, false),
                    None => (
                        h.ja_hilt.clone(),
                        h.ja_blades.clone(),
                        Some(grip.hilt_local()),
                        true,
                    ),
                };
                notes.push(format!(
                    "hilt {} in {} (JA bolt on {})",
                    ir.name, h.hand.weapon, gla.bones[h.bolt.bone].name
                ));
                models.push(Placed {
                    ir,
                    offset: [0.0; 3],
                    facing: 0.0,
                    is_static: false,
                    attach: None,
                    hook: Some((0, grip.weapon)),
                    hook_local: local,
                    clip: None,
                });
                hilts.push((models.len() - 1, blades, ja, Some((grip.weapon, local))));
            }
        }
        "b" => {
            let scale = head_scale(&body, &gla);
            notes.push(format!(
                "kotor meshes scaled by 1/{scale:.3} onto JA's skeleton"
            ));
            let mut ir = asset_ja::model(&glm, &gla, &BTreeMap::new());
            ir.name = format!("{} on _humanoid", options.body);
            ir.meshes = asset_ja::rig::rebind(
                &gla,
                &[
                    (&body, asset_odyssey::model::IDENTITY, None),
                    (&head, hook, Some(hook_node)),
                ],
                scale,
            );
            // Ground travel on the root bone, in JA space (JA faces −Y).
            let mut combo = combo;
            if let Some(track) = combo.tracks.first_mut() {
                for key in &mut track.positions {
                    let moved = ground.offset(key.0);
                    key.1[0] -= moved.x;
                    key.1[1] -= moved.y;
                    key.1[2] += moved.z;
                }
            }
            let clip = Some(Arc::new(combo));
            // JA models face −Y; half a turn faces them the way KOTOR's do.
            let facing = std::f32::consts::PI;
            let floor = [0.0, 0.0, asset_ja::ORIGIN_HEIGHT];
            models.push(Placed {
                ir,
                offset: floor,
                facing,
                is_static: false,
                attach: None,
                hook: None,
                hook_local: None,
                clip,
            });
            for h in &held {
                models.push(Placed {
                    ir: h.ja_hilt.clone(),
                    offset: [0.0; 3],
                    facing,
                    is_static: false,
                    attach: None,
                    hook: Some((0, h.bolt.bone + 1)),
                    hook_local: Some(h.bolt.matrix),
                    clip: None,
                });
                hilts.push((models.len() - 1, h.ja_blades.clone(), true, None));
            }
        }
        other => return Err(format!("{other}: use a, b or c")),
    }
    // The ground path in the model's clip time, for tracking shots.
    let track: Vec<[f32; 4]> = ground
        .samples(ja_length, 0.05)
        .into_iter()
        .map(|(t, p)| [t + ja_start, p.x, p.y, p.z].map(|v| (v * 1000.0).round() / 1000.0))
        .collect();
    notes.push(format!(
        "ground track: {}",
        serde_json::Value::from(track.iter().map(|r| r.to_vec()).collect::<Vec<_>>())
    ));
    // Thrown sabers, flown from and back to the right hand as it moves.
    let mut flights = Vec::new();
    if let (Some(clip), Some((_, _, _, Some((weapon, local))))) = (&traced, hilts.first()) {
        let hand = |ms: f32| {
            let pose = asset_ja::rig::pose_at(&models[0].ir, clip, ja_start + ms / 1000.0);
            let m = match local {
                Some(local) => asset_odyssey::model::multiply(&pose[*weapon], local),
                None => pose[*weapon],
            };
            Vec3::new(m[0][3], m[1][3], m[2][3])
        };
        let origin = |ms: f32| ground.offset(ms / 1000.0) + Vec3::Z * asset_ja::ORIGIN_HEIGHT;
        for throw in &throws {
            let flight = saber_flight::fly(throw, &hand, &origin, Vec3::Y, options.quirks);
            notes.push(format!(
                "saber thrown at {:.0} ms, let go at {:.0}, caught at {:.0}, {:.2} m out at most",
                throw.start,
                throw.release,
                flight.catch,
                (0..=((flight.catch - throw.start) / 25.0) as usize)
                    .map(|i| {
                        let ms = throw.start + i as f32 * 25.0;
                        flight.position(ms).distance(hand(ms))
                    })
                    .fold(0.0, f32::max)
            ));
            flights.push((flight, origin));
        }
    }
    let caught: Vec<f32> = flights.iter().map(|(f, _)| f.catch).collect();
    // The force powers' effects, from the retargeted hands.
    let mut effects = viewer::Effects::default();
    if let Some(clip) = &traced {
        let body = &models[0].ir;
        let node = |name: &str| body.node_index(name);
        let at = |node: Option<usize>| {
            move |ms: f32| {
                node.map_or(Vec3::ZERO, |n| {
                    let m = asset_ja::rig::pose_at(body, clip, ja_start + ms / 1000.0)[n];
                    Vec3::new(m[0][3], m[1][3], m[2][3])
                })
            }
        };
        let (left, right) = (at(node(hooks::LEFT_HAND)), at(node(hooks::RIGHT_HAND)));
        let flown: Vec<(f32, f32)> = flights.iter().map(|(f, _)| (f.start, f.catch)).collect();
        let player = force_fx::Player {
            left_hand: &left,
            right_hand: &right,
            angles: &angles,
            flights: &flown,
        };
        effects = force_fx::effects(
            &files,
            &powers,
            &player,
            (ja_start * 1000.0, period * 1000.0),
            &mut notes,
        );
        effects.blends = force_fx::blends(&files, &effects, &mut notes);
        if effects
            .items
            .iter()
            .any(|e| matches!(e, viewer::Effect::Refraction { .. }))
        {
            effects.refraction_model = refraction_model(&files, &mut notes);
        }
    }
    // The sabers: lit and put out as the timeline says, in KOTOR's clips
    // not there at all (KOTOR shows no weapon out of combat), and away from
    // the hand while thrown.
    let away: Vec<(f32, f32)> = flights
        .iter()
        .map(|(f, _)| (ja_start + f.start / 1000.0, ja_start + f.catch / 1000.0))
        .collect();
    let mut shown = Vec::new();
    let mut blades = Vec::new();
    let mut trail_keys = 0;
    for (index, (model, hilt_blades, _, weapon)) in hilts.iter().enumerate() {
        let gone: &[(f32, f32)] = if index == 0 { &away } else { &[] };
        let whole = if ja_start > 0.0 {
            (ja_start, ja_start + ja_length)
        } else {
            (0.0, period)
        };
        let ranges = without(whole, gone);
        // A hilt shown only part of the time takes its blades with it.
        let limited = ja_start > 0.0 || !gone.is_empty();
        if limited {
            shown.push(Shown {
                model: *model,
                ranges: ranges.clone(),
                period,
                path: Vec::new(),
            });
        }
        for blade in hilt_blades {
            let extend: Vec<(f32, f32)> = if saber.is_empty() {
                Vec::new()
            } else {
                let frame_ms = 1000.0 / 60.0;
                asset_ja::blade_lengths(&saber, blade.length, ja_length * 1000.0, frame_ms)
                    .into_iter()
                    .map(|(t, l)| (t + ja_start, l))
                    .collect()
            };
            let trail = match (&traced, weapon) {
                (Some(clip), Some((node, local))) => blade_trail(
                    &models[0].ir,
                    clip,
                    *node,
                    *local,
                    blade,
                    &extend,
                    &moves,
                    ja_start,
                ),
                _ => Vec::new(),
            };
            trail_keys += trail.iter().filter(|k| k.life_ms > 0.0).count();
            blades.push(BladeSpec {
                model: *model,
                extend,
                period,
                trail,
                // (An empty list would draw it always.)
                shown: match (limited, ranges.is_empty()) {
                    (false, _) => Vec::new(),
                    (true, false) => ranges.clone(),
                    (true, true) => vec![(0.0, 0.0)],
                },
                ..blade.clone()
            });
        }
    }
    // Each thrown saber: the right hand's hilt again, flying on its own,
    // blades full and trailing (`SABER_TRAIL_TIME` while in flight).
    let mut flying_ja = Vec::new();
    if let Some((hand_model, hand_blades, ja, _)) = hilts.first() {
        for ((flight, origin), &(from, to)) in flights.iter().zip(&away) {
            let step = 1.0 / 60.0;
            let times: Vec<f32> = (0..)
                .map(|i| from + i as f32 * step)
                .take_while(|&t| t < to)
                .chain([to])
                .collect();
            let pose = |t: f32| {
                let ms = (t - ja_start) * 1000.0;
                (flight.position(ms), flight.rotation(ms, origin(ms)))
            };
            let path = times
                .iter()
                .map(|&t| {
                    let (p, q) = pose(t);
                    (t, matrix(p, q))
                })
                .collect();
            let model = models.len();
            models.push(Placed {
                ir: models[*hand_model].ir.clone(),
                offset: [0.0; 3],
                facing: 0.0,
                is_static: false,
                attach: None,
                hook: None,
                hook_local: None,
                clip: None,
            });
            if *ja {
                flying_ja.push(model);
            }
            shown.push(Shown {
                model,
                ranges: vec![(from, to)],
                period,
                path,
            });
            for blade in hand_blades {
                let origin = Vec3::from_array(blade.origin);
                let dir = Vec3::from_array(blade.dir).normalize_or_zero();
                let tip = origin + dir * (blade.length + 3.0) * asset_ja::UNIT;
                let trail: Vec<viewer::TrailKey> = times
                    .iter()
                    .enumerate()
                    .map(|(i, &t)| {
                        let (p, q) = pose(t);
                        viewer::TrailKey {
                            time: t,
                            base: (p + q * origin).to_array(),
                            tip: (p + q * tip).to_array(),
                            life_ms: if i == 0 { 0.0 } else { 40.0 },
                        }
                    })
                    .collect();
                trail_keys += trail.len().saturating_sub(1);
                blades.push(BladeSpec {
                    model,
                    extend: Vec::new(),
                    period,
                    trail,
                    shown: vec![(from, to)],
                    ..blade.clone()
                });
            }
        }
    }
    notes.push(format!(
        "blades: {}, trail segments {trail_keys}",
        blades.len()
    ));
    // KOTOR textures for the body, head and KOTOR hilts; JA's for JA hilts
    // and the blades.
    let ja_models: Vec<usize> = hilts
        .iter()
        .filter(|h| h.2)
        .map(|h| h.0)
        .chain(flying_ja)
        .collect();
    let (mut textures, mut missing) = viewer::textures(
        &resolver,
        models
            .iter()
            .enumerate()
            .filter(|(i, _)| !ja_models.contains(i))
            .map(|(_, m)| m),
    );
    let (ja_textures, ja_missing) = viewer_ja::textures(
        &files,
        &viewer_ja::scene_textures(
            models
                .iter()
                .enumerate()
                .filter(|(i, _)| ja_models.contains(i))
                .map(|(_, m)| m),
            &blades,
        ),
    );
    textures.extend(ja_textures);
    missing.extend(ja_missing);
    let effect_shaders: Vec<String> = effects.blends.keys().cloned().collect();
    let (fx_textures, fx_missing) = viewer_ja::textures(&files, &effect_shaders);
    textures.extend(fx_textures);
    missing.extend(fx_missing);
    // A floor to judge the feet against: 1 m checks at z 0.
    let (floor, floor_texture) = floor();
    textures.insert(FLOOR_TEXTURE.to_string(), floor_texture);
    models.push(Placed {
        ir: floor,
        offset: [0.0; 3],
        facing: 0.0,
        is_static: true,
        attach: None,
        hook: None,
        hook_local: None,
        clip: None,
    });
    let scene = LoadedScene {
        name: format!("rig {} {}", options.probe, options.body),
        models,
        textures,
        missing,
        missing_body_textures: Vec::new(),
        entry: None,
        missing_models: Vec::new(),
        rooms: Vec::new(),
        own_fx: Vec::new(),
        instances: 0,
        creatures: 1,
        creature_refusals: Vec::new(),
        missing_clips: Vec::new(),
        creature_notes: notes,
        weapons: 0,
        ambient: [0.2; 3],
        fog: None,
        cameras: Vec::new(),
        map: None,
        cutscene_models: BTreeMap::new(),
        lit: false,
        blades,
        shown,
        effects,
        doors: Vec::new(),
        instance_models: BTreeMap::new(),
        creature_views: Vec::new(),
        arms: BTreeMap::new(),
        ..LoadedScene::default()
    };
    Ok((scene, assumed, caught))
}

/// JA's distortion model (`models/weaphits/testboom.md3`), its surfaces
/// in one list.
pub(crate) fn refraction_model(
    files: &Files,
    notes: &mut Vec<String>,
) -> Option<viewer::EffectModel> {
    let path = "models/weaphits/testboom.md3";
    let parsed = files
        .read(path)
        .ok_or(format!("{path}: not found"))
        .and_then(|bytes| fmt_ja::md3::parse(&bytes).map_err(|e| format!("{path}: {e}")));
    let md3 = match parsed {
        Ok(md3) => md3,
        Err(error) => {
            notes.push(error);
            return None;
        }
    };
    let mut model: viewer::EffectModel = (Vec::new(), Vec::new(), Vec::new());
    for surface in &md3.surfaces {
        let base = model.0.len() as u32;
        model.0.extend_from_slice(&surface.positions);
        model.1.extend_from_slice(&surface.uvs);
        model
            .2
            .extend(surface.triangles.iter().map(|t| t.map(|i| i + base)));
    }
    let (low, high) = model
        .0
        .iter()
        .fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            (
                [0, 1, 2].map(|i| lo[i].min(p[i])),
                [0, 1, 2].map(|i| hi[i].max(p[i])),
            )
        });
    notes.push(format!(
        "{path}: {} surfaces, {} triangles, from {low:.1?} to {high:.1?}",
        md3.surfaces.len(),
        model.2.len()
    ));
    Some(model)
}

/// `whole` less the stretches in `gone` (sorted, inside it).
fn without(whole: (f32, f32), gone: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut ranges = Vec::new();
    let mut from = whole.0;
    for &(start, end) in gone {
        if start > from {
            ranges.push((from, start));
        }
        from = from.max(end);
    }
    if whole.1 > from {
        ranges.push((from, whole.1));
    }
    ranges
}

/// A transform (K1's row-major matrix) from a turn and a position.
fn matrix(position: Vec3, rotation: bevy::math::Quat) -> asset_odyssey::model::Matrix {
    let m = bevy::math::Mat3::from_quat(rotation);
    [
        [m.x_axis.x, m.y_axis.x, m.z_axis.x, position.x],
        [m.x_axis.y, m.y_axis.y, m.z_axis.y, position.y],
        [m.x_axis.z, m.y_axis.z, m.z_axis.z, position.z],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Seconds between the KOTOR clip and the combo in probe `c`, which the
/// keys are interpolated across (KOTOR clips blend in over their
/// `transition`, 0.25 s on the humanoid clips).
const BLEND: f32 = 0.25;

/// How much of the KOTOR clip probe `c` plays before and after.
const KOTOR_SECONDS: f32 = 1.5;

/// The blade's path at 60 frames a second (JA emits a trail segment per
/// client frame; Q-0005's frame rate), for the model playing `clip` with
/// the hilt at `weapon` (under `hook_local`): the blade's base, and its tip
/// 3 units past its current length as JA draws it, with each
/// segment's life from the torso move playing then (`saber_trail_ms`).
#[allow(clippy::too_many_arguments)]
fn blade_trail(
    body: &ModelIr,
    clip: &asset_odyssey::anim::AnimationIr,
    weapon: usize,
    hook_local: Option<asset_odyssey::model::Matrix>,
    blade: &BladeSpec,
    extend: &[(f32, f32)],
    moves: &[(f32, String)],
    ja_start: f32,
) -> Vec<viewer::TrailKey> {
    let frame = 1.0 / 60.0;
    let steps = (clip.length / frame).floor() as usize;
    let mul = asset_odyssey::model::multiply;
    let point = |m: &asset_odyssey::model::Matrix, p: Vec3| {
        [0, 1, 2].map(|r| m[r][0] * p.x + m[r][1] * p.y + m[r][2] * p.z + m[r][3])
    };
    let extend_keys: Vec<(f32, [f32; 3])> =
        extend.iter().map(|&(t, l)| (t, [l, 0.0, 0.0])).collect();
    (0..=steps)
        .map(|step| {
            let time = step as f32 * frame;
            let pose = asset_ja::rig::pose_at(body, clip, time);
            let hilt = match &hook_local {
                Some(local) => mul(&pose[weapon], local),
                None => pose[weapon],
            };
            let length = asset_odyssey::anim::sample_position(&extend_keys, time)
                .map_or(blade.length, |v| v[0]);
            let origin = Vec3::from_array(blade.origin);
            let dir = Vec3::from_array(blade.dir).normalize_or_zero();
            let ms = (time - ja_start) * 1000.0;
            let playing = moves.iter().rfind(|(at, _)| *at <= ms);
            let life_ms = match playing {
                Some((_, name)) if ms >= 0.0 && length >= 0.5 => asset_ja::saber_trail_ms(name),
                _ => 0.0,
            };
            viewer::TrailKey {
                time,
                base: point(&hilt, origin),
                tip: point(&hilt, origin + dir * (length + 3.0) * asset_ja::UNIT),
                life_ms,
            }
        })
        .collect()
}

const FLOOR_TEXTURE: &str = "kjk_floor";

/// A 60 m square floor at z 0 with 1 m checks, and its texture.
fn floor() -> (ModelIr, asset_odyssey::texture::TextureIr) {
    use asset_odyssey::model::{IDENTITY, MeshIr, NodeIr};
    let half = 30.0;
    let ir = ModelIr {
        name: "floor".into(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: "floor".into(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes: vec![MeshIr {
            node: 0,
            positions: vec![
                [-half, -half, 0.0],
                [half, -half, 0.0],
                [half, half, 0.0],
                [-half, half, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uv1: vec![[0.0, 0.0], [half, 0.0], [half, half], [0.0, half]],
            uv2: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: Some(FLOOR_TEXTURE.into()),
            lightmap: None,
            transparency_hint: 0,
            skinned: false,
            self_illum: [0.0; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
        }],
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
    };
    // Two 1 m checks per texture repeat (the quad's UVs run half a unit a
    // metre).
    let size = 64u32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let light = ((x < size / 2) ^ (y < size / 2)) as u8;
            let grey = if light == 1 { 92 } else { 70 };
            pixels.extend([grey, grey, grey + 6, 255]);
        }
    }
    let texture = asset_odyssey::texture::TextureIr {
        width: size,
        height: size,
        encoding: asset_odyssey::texture::Encoding::Rgba8,
        mips: vec![pixels],
        cube: false,
        frames: 1,
        facts: asset_odyssey::texture::TextureFacts::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    };
    (ir, texture)
}

/// The lowest vertex of a GLM in its base pose (metres): where its soles
/// stand.
pub(crate) fn sole_height(glm: &fmt_ja::ghoul2::Glm) -> f32 {
    let low = glm
        .lods
        .first()
        .into_iter()
        .flatten()
        .flat_map(|surface| surface.positions.iter().map(|p| p[2]))
        .fold(f32::MAX, f32::min);
    if low == f32::MAX {
        0.0
    } else {
        low * asset_ja::UNIT
    }
}

/// KOTOR's finger nodes: `[lr][abcd]fngr[bt]_g` and `[lr]thumb[bt]_g`.
pub(crate) fn is_finger(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.len() > 2
        && (name.starts_with('r') || name.starts_with('l'))
        && name.ends_with("_g")
        && (name[2..].starts_with("fngr") || name[1..].starts_with("thumb"))
}

/// KOTOR's head height over JA's (`head_g` against `cranium`), so the
/// rebound body keeps JA's proportions at KOTOR's size.
fn head_scale(body: &ModelIr, gla: &fmt_ja::ghoul2::Gla) -> f32 {
    let kotor = body.node(hooks::HEAD).map(|node| node.world[2][3]);
    let ja = gla
        .bones
        .iter()
        .find(|bone| bone.name.eq_ignore_ascii_case("cranium"))
        .map(|bone| bone.base_pose[2][3] * asset_ja::UNIT);
    match (kotor, ja) {
        (Some(kotor), Some(ja)) if kotor > 0.0 && ja > 0.0 => kotor / ja,
        _ => 1.0,
    }
}

/// `first` then `second` on one model, the second starting `gap` seconds
/// after the first ends (keys interpolate across the gap). A node keyed in
/// only one of them holds its rest pose (offset 0, rest rotation) through
/// the other. Tracks the animator would not apply (`AnimationIr::bind`)
/// are dropped.
fn then(model: &ModelIr, first: &AnimationIr, second: &AnimationIr, gap: f32) -> AnimationIr {
    // The tracks the animator applies (`AnimationIr::bind`).
    fn kept<'a>(model: &ModelIr, clip: &'a AnimationIr) -> Vec<&'a NodeTrack> {
        let mut tracks: Vec<usize> = clip.bind(model).into_iter().flatten().collect();
        tracks.sort_unstable();
        tracks
            .into_iter()
            .map(|track| &clip.tracks[track])
            .collect()
    }
    let (firsts, seconds) = (kept(model, first), kept(model, second));
    let mut names: Vec<String> = firsts
        .iter()
        .chain(&seconds)
        .map(|t| t.node.to_ascii_lowercase())
        .collect();
    names.sort();
    names.dedup();
    let start = first.length + gap;
    let tracks = names
        .into_iter()
        .map(|name| {
            let a = firsts.iter().find(|t| t.node.eq_ignore_ascii_case(&name));
            let b = seconds.iter().find(|t| t.node.eq_ignore_ascii_case(&name));
            let rest_rotation = model
                .node(&name)
                .map_or([0.0, 0.0, 0.0, 1.0], |node| node.rotation);
            NodeTrack {
                node: name.clone(),
                positions: join(
                    a.map(|t| t.positions.as_slice()),
                    b.map(|t| t.positions.as_slice()),
                    [0.0; 3],
                    (first.length, start, second.length),
                ),
                orientations: join(
                    a.map(|t| t.orientations.as_slice()),
                    b.map(|t| t.orientations.as_slice()),
                    rest_rotation,
                    (first.length, start, second.length),
                ),
            }
        })
        .collect();
    AnimationIr {
        name: format!("{}+{}", first.name, second.name),
        length: start + second.length,
        transition: 0.0,
        root: None,
        tracks,
        settings: Vec::new(),
        events: Vec::new(),
    }
}

/// One track's keys from two clips (`then`): the first's keys, then the
/// second's from `start`; a clip without keys holds `rest` at its ends.
/// Empty when neither has keys. `times` is (first length, start, second
/// length).
fn join<const N: usize>(
    a: Option<&[(f32, [f32; N])]>,
    b: Option<&[(f32, [f32; N])]>,
    rest: [f32; N],
    (first, start, second): (f32, f32, f32),
) -> Vec<(f32, [f32; N])> {
    let a = a.filter(|keys| !keys.is_empty());
    let b = b.filter(|keys| !keys.is_empty());
    if a.is_none() && b.is_none() {
        return Vec::new();
    }
    let mut keys = Vec::new();
    match a {
        Some(a) => keys.extend_from_slice(a),
        None => keys.extend([(0.0, rest), (first, rest)]),
    }
    match b {
        Some(b) => keys.extend(b.iter().map(|&(t, v)| (start + t, v))),
        None => keys.extend([(start, rest), (start + second, rest)]),
    }
    keys
}
