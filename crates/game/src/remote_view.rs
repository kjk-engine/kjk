//! The seats' remote droids drawn (`docs/superpowers/specs/2026-10-29-remote-droid-design.md`):
//!
//! - **Its model**: TSL's remote (Bao-Dur's, `p_remote.utc`), the model of
//!   the `appearance.2da` row labelled `Droid_SensorBall` in TSL's data (K2
//!   is required), found by label, not by number. It hovers where the sim
//!   has it, bobbing a little, turned to its seat's view (yaw and pitch);
//!   the own seat's where the view shows it (`play_view::OwnRemote`, its
//!   predictions), so it and the camera move together. Not drawn while it
//!   waits out a hold (`Remote::hidden`).
//! - **Its kit**: a press is seen as a button's ready time moving on
//!   (`Remote::ready_ms`), drawn with KOTOR's own: the shock bolt as the
//!   ion blaster's bolt (its tracer and shot sound, `blaster_view`) from
//!   the droid to where it stopped (`Remote::bolt_to`), with the blaster's
//!   sparks there; the Sonic Blast as the sonic grenade's blast
//!   (`VFX_FNF_GRENADE_SONIC`, `script_fx`) at the droid; the stim dart as
//!   `gui_inject`, its patient showing the medpac's heal (the sim's visual
//!   event).
//!
//! Its camera is `play_view`'s (`cast::remote_in`); its buttons are the HUD's
//! (`hud`).

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use sim::Seat;
use sim::remote::{KIT_BOLT, KIT_DART, KIT_SONIC, Remote};

use crate::blaster_view::{BlasterShots, Tracer};
use crate::fx_model::{Spawned, Spawning};
use crate::heat_haze::HeatHazeMaterial;
use crate::play_view::OwnRemote;
use crate::scene::{ModelAssets, SceneContent, SceneRoot, Spawner};
use crate::script_fx::{HookVisual, HookVisuals, Loaded, Place, Read};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap, view_ms};
use crate::sound::{Cue, Cues};
use crate::weapon_sounds::WeaponSounds;

const UNIT: f32 = collision::UNIT;
/// The droid's box, middle to side (metres): the model's lowest point sits
/// this far under the droid's place.
const HALF_SIZE_M: f32 = rules::remote::flight::HALF_SIZE_M;
/// TSL's remote's `appearance.2da` label.
pub const APPEARANCE: &str = "Droid_SensorBall";
/// How far it bobs (metres) and how often (bobs a second).
const BOB_M: f32 = 0.05;
const BOBS: f32 = 0.6;
/// The ion blaster's `baseitems.2da` row: the shock bolt is its bolt.
const ION_BLASTER: u16 = 15;
/// `visualeffects.2da`'s sonic grenade blast (`VFX_FNF_GRENADE_SONIC`),
/// and the blaster's sparks where a bolt stops (`VFX_COM_SPARKS_BLASTER`).
const SONIC_BLAST: u16 = 3007;
const SPARKS: u16 = 4033;
/// The stim dart's sound (a KOTOR resref, read with `power_fx`'s).
const DART_SOUND: &str = "k1/gui_inject";

pub struct RemoteViewPlugin;

impl Plugin for RemoteViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteView>().add_systems(
            PostUpdate,
            draw.run_if(resource_exists::<PresentedSnapshot>)
                .after(crate::play_view::Drawn)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// The droids drawn, their model, and each seat's kit as last seen.
#[derive(Resource, Default)]
pub struct RemoteView {
    generation: u32,
    model: Option<Loaded<Read>>,
    live: BTreeMap<Seat, Spawned>,
    ready: BTreeMap<Seat, [u64; 3]>,
}

/// TSL's remote and its textures, from TSL's own data.
fn read_remote() -> Read {
    let install = crate::installs::open(&crate::discover(), vfs::Game::Kotor2).ok()?;
    let resolver = install.base().ok()?;
    let table = resolver.two_da("appearance")?;
    let row = (0..table.cells.len())
        .find(|&row| table.text(row, "label").as_deref() == Some(APPEARANCE))?;
    let name = table.resref(row, "race")?;
    let model = crate::fx_model::load(resolver, &name, false)?;
    let textures = crate::fx_model::textures(resolver, std::iter::once(&model));
    Some((Arc::new(model), Arc::new(textures)))
}

/// Where `remote` is drawn (metres), `since_ms` past its snapshot: ahead
/// along its way.
fn drawn_at(remote: &Remote, since_ms: f32) -> Vec3 {
    let ahead = (since_ms / 1000.0).clamp(0.0, 0.1);
    Vec3::from_array(remote.at) + Vec3::from_array(remote.velocity) * ahead
}

/// `remote`'s place (metres) bobbing at `ms`.
fn bobbed(remote: &Remote, ms: f32) -> Vec3 {
    let bob = BOB_M * (std::f32::consts::TAU * BOBS * ms / 1000.0 + f32::from(remote.seat.0)).sin();
    Vec3::from_array(remote.at) + Vec3::Z * bob
}

/// Where the kit's presses go: their sounds, the bolt's tracer, the
/// effects at a place; and the blasters' table.
type Kit<'w> = (
    ResMut<'w, Cues>,
    ResMut<'w, BlasterShots>,
    Option<ResMut<'w, HookVisuals>>,
    Option<Res<'w, WeaponSounds>>,
);

#[allow(clippy::too_many_arguments)]
fn draw(
    mut view: ResMut<RemoteView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    roots: Query<Entity, With<SceneRoot>>,
    (mut assets, mut hazes): (ModelAssets, ResMut<Assets<HeatHazeMaterial>>),
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    seen: Query<&InheritedVisibility>,
    (own_seat, own_remote): (Option<Res<OwnSeat>>, Res<OwnRemote>),
    (mut cues, mut shots, mut hooks, kotor): Kit,
    (mut facts, dump_due): (ResMut<console::DumpFacts>, Res<console::DumpDue>),
    mut commands: Commands,
) {
    let view = &mut *view;
    if swap.generation != view.generation {
        view.generation = swap.generation;
        // Their entities went with the old scene; the model stays read.
        view.live.clear();
        view.ready.clear();
    }
    let current = presented.0.current();
    let flying: BTreeMap<Seat, Remote> = current
        .remotes
        .iter()
        .map(|remote| (remote.seat, *remote))
        .collect();
    let gone: Vec<Seat> = (view.live.keys())
        .filter(|seat| !flying.contains_key(seat))
        .copied()
        .collect();
    for seat in gone {
        if let Some(spawned) = view.live.remove(&seat) {
            spawned.despawn(&mut commands);
        }
    }
    view.ready.retain(|seat, _| flying.contains_key(seat));
    let now = view_ms(&presented.0) as f32;
    let since = now - current.time_ms as f32;
    let own_seat = own_seat.map_or(Seat(0), |seat| seat.0);
    // Where each droid is drawn (metres): the own from its predictions.
    let drawn: BTreeMap<Seat, Remote> = flying
        .iter()
        .map(|(seat, remote)| {
            let shown = match own_remote.0 {
                Some(own) if *seat == own_seat => own,
                _ => Remote {
                    at: drawn_at(remote, since).to_array(),
                    ..*remote
                },
            };
            (*seat, shown)
        })
        .collect();
    // A press: KOTOR's effect for its button.
    for (seat, remote) in &flying {
        let before = view.ready.insert(*seat, remote.ready_ms);
        let Some(before) = before else { continue };
        let from = Vec3::from_array(drawn[seat].at);
        let pressed = |kit: usize| remote.ready_ms[kit] > before[kit];
        if pressed(KIT_BOLT) {
            let to = Vec3::from_array(remote.bolt_to);
            let ammo = kotor.as_deref().and_then(|kotor| kotor.ammo(ION_BLASTER));
            if let Some(model) = ammo.and_then(|ammo| ammo.models[0].clone()) {
                shots.tracers.push(Tracer {
                    model,
                    from,
                    to,
                    ms: f64::from(now),
                });
            }
            if let Some(sound) = kotor.as_deref().and_then(|k| k.shot(ION_BLASTER, false)) {
                cues.0.push(Cue::at(sound, from / UNIT, now));
            }
            if let Some(hooks) = hooks.as_deref_mut() {
                hooks.0.push(HookVisual {
                    row: SPARKS,
                    place: Place::At(to),
                });
            }
        }
        if pressed(KIT_SONIC)
            && let Some(hooks) = hooks.as_deref_mut()
        {
            hooks.0.push(HookVisual {
                row: SONIC_BLAST,
                place: Place::At(from),
            });
        }
        if pressed(KIT_DART) {
            cues.0
                .push(Cue::at(DART_SOUND.to_string(), from / UNIT, now));
        }
    }
    let model = view.model.get_or_insert_with(|| {
        Loaded::Loading(AsyncComputeTaskPool::get().spawn(async { read_remote() }))
    });
    let model = model.poll().cloned().flatten();
    if let (Some((model, textures)), Some(root)) = (&model, roots.iter().next()) {
        for seat in flying.keys() {
            if view.live.contains_key(seat) {
                continue;
            }
            let (images, stores) = assets.split();
            let mut spawning = Spawning {
                spawner: Spawner::new(textures, scene.0.area_light(), images),
                stores,
                hazes: &mut hazes,
                textures,
                area: scene.0.area_light(),
                lit: scene.0.lit,
                root,
                seed: u32::from(seat.0) | 1,
                commands: &mut commands,
            };
            view.live.insert(*seat, spawning.spawn(model.copy(), true));
        }
    }
    let mut shown = Vec::new();
    for (seat, spawned) in &view.live {
        let Some(remote) = drawn.get(seat) else {
            continue;
        };
        // The model stands on its root as a creature does (TSL's remote
        // floats a metre and more over it): its middle where the droid is.
        let at = bobbed(remote, now) - Vec3::Z * (spawned.model.floor + HALF_SIZE_M);
        if let Ok(mut transform) = transforms.get_mut(spawned.root) {
            // Turned to its yaw, then tilted to its pitch (down positive)
            // about its middle.
            let middle = Vec3::Z * (spawned.model.floor + HALF_SIZE_M);
            let turned = Quat::from_rotation_z(remote.yaw.to_radians())
                * Quat::from_rotation_y(remote.pitch.to_radians());
            *transform =
                Transform::from_translation(at + middle - turned * middle).with_rotation(turned);
        }
        if let Ok(mut seen) = visibility.get_mut(spawned.root) {
            *seen = if remote.hidden {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
        shown.push(serde_json::json!({
            "seat": seat.0,
            "floor_m": spawned.model.floor,
            "visible": seen.get(spawned.root).ok().map(|seen| seen.get()),
            "at_m": at.to_array(),
            "hidden": remote.hidden,
            "ready_ms": remote.ready_ms,
        }));
    }
    if dump_due.0 {
        facts.0.insert(
            "remotes".into(),
            serde_json::json!({
                "model": model.as_ref().map(|(model, _)| model.placed.ir.name.clone()),
                "in_snapshot": flying.keys().map(|seat| seat.0).collect::<Vec<_>>(),
                "drawn": shown,
            }),
        );
    }
}
