//! Barks in `kjk play` (render side only: the presented snapshots' barks,
//! `Told::barks`): one-liners (`SpeakOneLinerConversation`, a conversation
//! of one line) and scripts' `BarkString`.
//!
//! - The latest bark shows in KOTOR's bark bubble (`ui::screens::bark`)
//!   for as long as the sim keeps it, its words as a conversation's are
//!   shown (`dialogue_view::said`): over its speaker's head (its head's
//!   node, else 2 m over where it stands) while the speaker is drawn and in
//!   front of the camera, else where the layout puts it (nobody speaks: the
//!   Ebon Hawk's turret's "Incoming fighters!"; or a minigame hides
//!   everyone).
//! - Its voice plays once, from when it was barked (`Bark::began_ms`): an
//!   entry's as its conversation's line would, a talk-table line's its
//!   entry's sound (K1's `BarkString` lines name theirs:
//!   `nm12aacuts01001_`); from a drawn speaker, following it, else heard
//!   alike everywhere; barks alike at once are voiced once. It plays out
//!   even past the bubble: a talk-table line's bark lasts 4 s, its voice
//!   as long as it is.

use audio::{Channel, Falloff};
use bevy::prelude::*;
use fmt_odyssey::tlk::SOUND_PRESENT;
use script_ncs::vm::ObjectId;
use sim::script::dialogue::{Bark, Said};
use sim::{SimId, Snapshot};

use crate::cast::Hitboxes;
use crate::dialogue_view::{Fill, Screen, node_text, said, speaker_at};
use crate::lifecycle_view::SceneCamera;
use crate::scene::game_to_bevy;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::sound::{Added, Cue, Cues, Forget};
use crate::ui_view::Ui;

/// How far over where a speaker stands its bubble hangs (metres): a fixed
/// height, not its head's node, which sways with its walk and its breath.
const OVER_FEET: f32 = 2.0;
/// How fast the bubble follows its speaker on the screen: the time it takes
/// to cover most of the way (seconds), so a walk moves it smoothly.
const FOLLOW_S: f32 = 0.12;

pub struct BarkViewPlugin;

impl Plugin for BarkViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            show.run_if(resource_exists::<PresentedSnapshot>)
                .before(crate::ui_view::Stepped),
        );
    }
}

/// The bark's words and the voice it names, as the game's bytes.
fn words(bark: &Bark, swap: &SceneSwap, screen: &Screen, fill: Fill) -> (Vec<u8>, Option<String>) {
    let tlk = screen.tlk.as_deref();
    match &bark.text {
        Said::Entry { dialog, entry } => {
            let library = swap.level.scripts();
            let node = library
                .and_then(|library| library.dialogue(dialog))
                .and_then(|dlg| dlg.entries.get(*entry as usize));
            let Some(node) = node else {
                return (Vec::new(), None);
            };
            let strref = |strref| tlk?.text(strref).map(<[u8]>::to_vec);
            let voice = library
                .and_then(|library| library.voice(node))
                .map(|(name, _)| name.to_string());
            (node_text(node, &strref, fill), voice)
        }
        Said::StrRef(strref) => {
            let entry = tlk.and_then(|tlk| tlk.entries.get(*strref as usize));
            let text = tlk.and_then(|tlk| tlk.text(*strref)).unwrap_or(&[]);
            let voice = entry
                .filter(|entry| entry.flags & SOUND_PRESENT != 0 && !entry.sound.is_empty())
                .map(|entry| entry.sound.clone());
            (said(text, fill), voice)
        }
    }
}

/// The speaker of `who`, while it is drawn (no minigame hides it).
fn drawn(snapshot: &Snapshot, who: ObjectId) -> Option<SimId> {
    let id = SimId(who.0);
    (snapshot.minigame.is_none() && snapshot.entities.iter().any(|e| e.id == id)).then_some(id)
}

/// The point `lift` metres over `id`'s head's node (else `OVER_FEET` over
/// where it stands) as shares of the window across and down, while it is
/// in front of the camera and on the screen.
pub(crate) fn over(
    snapshot: &Snapshot,
    id: SimId,
    lift: f32,
    hitboxes: Option<&Hitboxes>,
    places: &Query<&GlobalTransform>,
    camera: (&GlobalTransform, &Camera),
) -> Option<[f32; 2]> {
    let entity = snapshot.entities.iter().find(|e| e.id == id)?;
    let head = hitboxes
        .and_then(|boxes| boxes.0.get(&id))
        .and_then(|spots| spots[sim::combat::Location::Head as usize])
        .and_then(|node| places.get(node).ok())
        .map(|place| place.translation() + Vec3::Y * lift);
    let at = head.unwrap_or_else(|| {
        game_to_bevy() * (Vec3::from_array(entity.position) * collision::UNIT) + Vec3::Y * OVER_FEET
    });
    let (place, camera) = camera;
    let share = camera.world_to_viewport(place, at).ok()? / camera.logical_viewport_size()?;
    let on = |v: f32| (0.0..=1.0).contains(&v);
    (on(share.x) && on(share.y)).then_some([share.x, share.y])
}

#[allow(clippy::too_many_arguments)]
fn show(
    presented: Res<PresentedSnapshot>,
    swap: Option<Res<SceneSwap>>,
    screen: Option<ResMut<Screen>>,
    places: Query<&GlobalTransform>,
    cameras: Query<(&GlobalTransform, &Camera), SceneCamera>,
    mut ui: ResMut<Ui>,
    (mut cues, mut added, mut forget): (ResMut<Cues>, ResMut<Added>, ResMut<Forget>),
    mut heard: Local<Vec<Bark>>,
    time: Res<Time>,
    mut followed: Local<Option<(Bark, [f32; 2])>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let snapshot = presented.0.current();
    let (Some(swap), Some(mut screen)) = (swap, screen) else {
        ui.vm.bark = None;
        return;
    };
    let told = &snapshot.talk;
    // Said to the first seat's player (a bark has no listener).
    let fill = Fill {
        told,
        pc: sim::persona::of(&snapshot.personas, sim::Seat(0)),
    };
    // One voice for barks alike (the turret's skybox barks its line twice
    // at once).
    let new = told
        .barks
        .iter()
        .enumerate()
        .filter(|(at, bark)| !heard.contains(bark) && !told.barks[..*at].contains(bark));
    for (_, bark) in new {
        let (_, voice) = words(bark, &swap, &screen, fill);
        let (Some(name), Some(library)) = (voice, swap.level.scripts()) else {
            continue;
        };
        let Some(cue) = screen.voice_cue(&swap, &name, &mut added, &mut forget) else {
            continue;
        };
        let ms = bark.began_ms(library) as f32;
        cues.0.push(match drawn(snapshot, bark.who) {
            Some(speaker) => Cue {
                entity: Some(speaker),
                channel: Channel::Voice,
                follows: true,
                ..Cue::at(cue, speaker_at(snapshot, speaker), ms)
            },
            None => Cue {
                falloff: Falloff::Everywhere,
                ..Cue::at(cue, Vec3::ZERO, ms)
            },
        });
    }
    heard.clone_from(&told.barks);
    let camera = cameras.single().ok();
    let shown = told.barks.last().map(|bark| {
        let (text, voice) = words(bark, &swap, &screen, fill);
        let target = drawn(snapshot, bark.who)
            .zip(camera)
            .and_then(|(id, camera)| over(snapshot, id, 0.0, None, &places, camera));
        // The same bubble eases toward where its speaker is; a new one, or
        // one come back on the screen, starts there.
        let over = target.map(|target| {
            let at = match followed.as_ref() {
                Some((was, at)) if was == bark => {
                    let ease = 1.0 - (-time.delta_secs() / FOLLOW_S).exp();
                    [
                        at[0] + (target[0] - at[0]) * ease,
                        at[1] + (target[1] - at[1]) * ease,
                    ]
                }
                _ => target,
            };
            *followed = Some((bark.clone(), at));
            at
        });
        if over.is_none() {
            *followed = None;
        }
        (ui::vm::BarkView { text, over }, voice)
    });
    match &shown {
        Some((view, voice)) => facts.0.insert(
            "bark".into(),
            serde_json::json!({
                "said": view.text.iter().map(|&b| char::from(b)).collect::<String>(),
                "over": view.over,
                "voice": voice,
            }),
        ),
        None => facts.0.remove("bark"),
    };
    ui.vm.bark = shown.map(|(view, _)| view);
}
