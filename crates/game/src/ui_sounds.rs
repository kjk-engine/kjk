//! KOTOR's interface sounds.
//!
//! - The screens' (`ui::intent::GuiSound`, `Frame::sounds`), each its
//!   `guisounds.2da` row's `soundresref` (the row found by its label), as
//!   K1's executable plays them: every button and list row (its controls
//!   are buttons) clicks `Clicked_Default`, a check box `Checkbox_Check`;
//!   the focus or the pointer coming to one `Entered_Default`; the first
//!   in-game screen opened `Fullscreen_Panel_Added`, the last closed
//!   `…_Removed`; the store's purchase `Inventory_Add`, sale
//!   `Inventory_Drop`; the equip screen's item worn `Inventory_Select`,
//!   taken off `Inventory_Drop`; and a party member come to a level
//!   `Level_Up_Notify` (the HUD's mark lit). Heard at once, alike
//!   everywhere, over a break or a movie too (`sound::Now`); the files read
//!   from the screens' install as first played, so the menu has them.
//!
//! - The journal (`guisounds.2da`): `Quest_New_Notify` (`gui_quest`) as a
//!   quest moves on, `Quest_Complete_Notify` (`gui_complete`) as it reaches
//!   an end entry (`progress`).
//! - A skill's use (K1's exe, as a creature plays the engine's animation):
//!   `gui_lockpick` for Security on a door or a container (`unlockdr`,
//!   `unlockcntr`), `gui_minedisarm` for disarming or recovering a mine,
//!   `gui_minearm` for setting one (`skills`).
//! - Pazaak (K1's exe, `mgs_*`): a main-deck card drawn, a side card
//!   played, the player's turn, a set and the match won or lost
//!   (`pazaak_view`).
//!
//! KOTOR's sounds are named `k1/<resref>` (`sound::load_kotor`).

use std::collections::BTreeMap;

use audio::Falloff;
use bevy::prelude::*;
use sim::{Presented, SimId};

use crate::sim_host::{PresentedSnapshot, view_ms};
use crate::sound::{Added, Cue, Cues, Now};
use crate::ui_view::Ui;
use ui::intent::GuiSound;

pub const QUEST_NEW: &str = "gui_quest";
pub const QUEST_COMPLETE: &str = "gui_complete";
const LOCKPICK: &str = "gui_lockpick";
const MINE_DISARM: &str = "gui_minedisarm";
const MINE_ARM: &str = "gui_minearm";
pub const PAZAAK_DRAW: &str = "mgs_drawmain";
pub const PAZAAK_SIDE: &str = "mgs_playside";
pub const PAZAAK_TURN: &str = "mgs_startturn";
pub const PAZAAK_SET_WON: &str = "mgs_winset";
pub const PAZAAK_SET_LOST: &str = "mgs_loseset";
pub const PAZAAK_MATCH_WON: &str = "mgs_winmatch";
pub const PAZAAK_MATCH_LOST: &str = "mgs_losematch";

/// The engine's skill animations and the sound each plays.
const SKILLS: [(u16, &str); 5] = [
    (sim::locks::animations::DOOR, LOCKPICK),
    (sim::locks::animations::PLACEABLE, LOCKPICK),
    (sim::traps::animations::DISARM, MINE_DISARM),
    (sim::traps::animations::RECOVER, MINE_DISARM),
    (sim::traps::animations::SET, MINE_ARM),
];

/// An animation begun longer ago than this (ms) is not sounded: one seen
/// first on a load or a join.
const FRESH_MS: u64 = 500;

/// The resrefs this plays, for the bank.
pub fn resrefs() -> Vec<&'static str> {
    let mut resrefs = vec![
        QUEST_NEW,
        QUEST_COMPLETE,
        PAZAAK_DRAW,
        PAZAAK_SIDE,
        PAZAAK_TURN,
        PAZAAK_SET_WON,
        PAZAAK_SET_LOST,
        PAZAAK_MATCH_WON,
        PAZAAK_MATCH_LOST,
    ];
    resrefs.extend(SKILLS.iter().map(|(_, sound)| *sound));
    resrefs.dedup();
    resrefs
}

/// `resref` heard now, wherever the listener is.
pub fn cue(presented: &Presented, resref: &str) -> Cue {
    Cue {
        falloff: Falloff::Everywhere,
        ..Cue::at(
            format!("k1/{resref}"),
            Vec3::ZERO,
            view_ms(presented) as f32,
        )
    }
}

pub struct UiSoundsPlugin;

impl Plugin for UiSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Heard>().add_systems(
            Update,
            (
                skills,
                (level_up, screens).chain().after(crate::ui_view::Stepped),
            ),
        );
    }
}

/// The screens' sounds this frame (`ui_view` gathers them).
#[derive(Resource, Default)]
pub struct Heard(pub Vec<GuiSound>);

/// A level-up mark lit where none was: `Level_Up_Notify`.
fn level_up(ui: Res<Ui>, mut heard: ResMut<Heard>, mut lit: Local<bool>) {
    let now = ui.vm.hud.is_some() && ui.vm.party.iter().any(|member| member.may_level);
    if now && !*lit {
        heard.0.push(GuiSound::LevelUp);
    }
    *lit = now;
}

/// What a sound's label names (`guisounds.2da`), read once.
#[derive(Default)]
struct Rows {
    resrefs: BTreeMap<GuiSound, Option<String>>,
    /// Those read into the bank.
    read: std::collections::BTreeSet<String>,
}

/// The screens' sounds, played now.
fn screens(
    mut ui: ResMut<Ui>,
    mut heard: ResMut<Heard>,
    (mut now, mut added): (ResMut<Now>, ResMut<Added>),
    mut rows: Local<Rows>,
) {
    if heard.0.is_empty() {
        return;
    }
    let Some(assets) = ui.assets.as_mut() else {
        heard.0.clear();
        return;
    };
    let rows = &mut *rows;
    for sound in heard.0.drain(..) {
        let resref = rows
            .resrefs
            .entry(sound)
            .or_insert_with(|| guisound(assets, sound.label()));
        let Some(resref) = resref.clone() else {
            continue;
        };
        let name = format!("k1/{resref}");
        if rows.read.insert(resref.clone()) {
            match crate::sound::kotor_file(assets.resolver(), &resref)
                .map(|bytes| audio::Clip::kotor(&bytes))
            {
                Some(Ok(clip)) => added.0.push((name.clone(), clip)),
                _ => warn!(target: "sound", "interface sound {resref} not read"),
            }
        }
        now.0.push(name);
    }
}

/// The `soundresref` of `guisounds.2da`'s row labelled `label`.
fn guisound(assets: &mut ui::assets::GuiAssets, label: &str) -> Option<String> {
    const TABLE: &str = "guisounds";
    let row = (0..assets.rows(TABLE)).find(|&row| {
        assets
            .cell(TABLE, row, "label")
            .is_some_and(|cell| cell.eq_ignore_ascii_case(label))
    })?;
    assets
        .cell(TABLE, row, "soundresref")
        .map(|resref| resref.to_ascii_lowercase())
}

/// A skill's sound where its user is, once as its animation begins.
fn skills(
    presented: Option<Res<PresentedSnapshot>>,
    cues: Option<ResMut<Cues>>,
    mut heard: Local<BTreeMap<SimId, u64>>,
) {
    let (Some(presented), Some(mut cues)) = (presented, cues) else {
        return;
    };
    let current = presented.0.current();
    let now = current.time_ms;
    heard.retain(|_, since| now < since.saturating_add(10_000));
    for entity in &current.entities {
        let Some(acting) = entity.fighter.and_then(|fighter| fighter.acting) else {
            continue;
        };
        let Some((_, sound)) = SKILLS
            .iter()
            .find(|(animation, _)| *animation == acting.animation)
        else {
            continue;
        };
        if !acting.playing(now)
            || now.saturating_sub(acting.since) > FRESH_MS
            || heard.get(&entity.id) == Some(&acting.since)
        {
            continue;
        }
        heard.insert(entity.id, acting.since);
        cues.0.push(Cue {
            entity: Some(entity.id),
            ..Cue::at(
                format!("k1/{sound}"),
                Vec3::from_array(entity.position) / asset_ja::UNIT,
                acting.since as f32,
            )
        });
    }
}
