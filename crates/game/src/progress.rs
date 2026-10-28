//! The story's progress on the HUD (I19), from the presented snapshot's
//! (`sim::Progress`): the level-up mark while the player's character may
//! take a level; the game's "Journal Updated:" with the quest's name in the
//! feedback log, with the journal's sound (`ui_sounds`); and KOTOR's
//! status summary (`ui::screens::status_summary`)
//! of what the scripts told the party (`Progress::statuses`).
//!
//! As KOTOR's client does, the statuses gather while a conversation, a fade
//! or a screen is up, and the summary shows a quarter second after none
//! is, the HUD's status icons lit with it for a few seconds. KOTOR's
//! summary pauses the game until OK; in real time it slides in at the right
//! edge and fades on its own.

use bevy::prelude::*;
use sim::StatusKind;
use ui::draw::Textures as _;
use ui::screens::status_summary::Row;
use ui::skin::Source;
use ui::vm::SummaryView;

use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// How long the HUD's status icons stay lit (s).
const LIT_S: f64 = 4.0;
/// How long nothing must hold the summary back before it shows (s).
const SETTLE_S: f64 = 0.25;
/// A summary's time coming in, staying, and fading out (s).
const IN_S: f64 = 0.25;
const SHOWN_S: f64 = 6.0;
const OUT_S: f64 = 0.6;
/// "Journal Updated:".
const JOURNAL_UPDATED: u32 = 32296;

/// The summary's lines: "Journal Entry Added", "Credits Received:
/// <CUSTOM0>", "Credits Lost: <CUSTOM0>", "Experience Points (XP)
/// Received: <CUSTOM0>", "Dark Side Points Gained", "Light Side Points
/// Gained", "Item(s) Received", "Item(s) Lost".
const JOURNAL_ADDED: u32 = 42436;
const CREDITS_RECEIVED: u32 = 42437;
const CREDITS_LOST: u32 = 42627;
const XP_RECEIVED: u32 = 42438;
const DARK_SIDE: u32 = 42440;
const LIGHT_SIDE: u32 = 42441;
const ITEMS_RECEIVED: u32 = 42442;
const ITEMS_LOST: u32 = 42443;
/// KOTOR 2's "Influence Gained: <CUSTOM0>" and "Influence Lost:
/// <CUSTOM0>", and its companions' names by `NPC_*` number, as its status
/// summary names them.
const INFLUENCE_GAINED: u32 = 48607;
const INFLUENCE_LOST: u32 = 48608;
/// KOTOR 2's "Maximum Force Points Increased" and "… Decreased".
const MAX_FP_GAINED: u32 = 48610;
const MAX_FP_LOST: u32 = 48611;
const COMPANIONS: [u32; 12] = [
    48391, 102045, 49456, 48392, 102046, 27766, 102048, 102049, 22095, 48384, 102047, 48393,
];
const CUSTOM: &[u8] = b"<CUSTOM0>";

/// What the views tell the summary themselves: statuses the sim raises
/// none for (a pazaak match's credits won or lost: `pazaak_view`), gathered
/// with the sim's; and notices of ours (a game saved: `save_view`), each a
/// summary of its own at once.
#[derive(Resource, Default)]
pub struct Told {
    pub statuses: Vec<StatusKind>,
    pub notices: Vec<String>,
}

/// What was last seen (where and when: a load or another module starts
/// over), the summary gathering, the HUD's icons lit, and the summaries up.
#[derive(Default)]
pub struct Seen {
    at: Option<(Option<String>, u64)>,
    journal: Option<Vec<(String, i32)>>,
    status_ms: Option<u64>,
    gathering: Summary,
    clear_since: Option<f64>,
    lit: Vec<&'static str>,
    lit_until: f64,
    up: Vec<(SummaryView, f64)>,
}

/// A summary as it gathers (`CStatusSummary`'s): each kind told, the XP and
/// credits added up, the items received by template, the companions whose
/// influence went up and down (bits by `NPC_*` number; one may be in both),
/// what the bonus Force points added to the gained and lost counts (each
/// line shown while its count is not 0, as KOTOR 2's).
#[derive(Default, PartialEq)]
struct Summary {
    journal: bool,
    xp: i32,
    credits: i32,
    dark: bool,
    light: bool,
    received: Vec<String>,
    lost: bool,
    influence_gained: u16,
    influence_lost: u16,
    max_fp_gained: i32,
    max_fp_lost: i32,
}

/// `text` with `value` where its `<CUSTOM0>` is, or `text` itself.
fn filled(text: Vec<u8>, value: &[u8]) -> Vec<u8> {
    match text.windows(CUSTOM.len()).position(|at| at == CUSTOM) {
        Some(at) => [&text[..at], value, &text[at + CUSTOM.len()..]].concat(),
        None => text,
    }
}

impl Summary {
    fn add(&mut self, kind: &StatusKind) {
        match kind {
            StatusKind::Journal => self.journal = true,
            StatusKind::Xp(xp) => self.xp += xp,
            StatusKind::Credits(credits) => self.credits += credits,
            StatusKind::Alignment { light: true } => self.light = true,
            StatusKind::Alignment { light: false } => self.dark = true,
            StatusKind::Received(template) => self.received.push(template.clone()),
            StatusKind::Lost => self.lost = true,
            StatusKind::Influence { npc, gained } => {
                let bit = 1u16.checked_shl(u32::from(*npc)).unwrap_or(0);
                match gained {
                    true => self.influence_gained |= bit,
                    false => self.influence_lost |= bit,
                }
            }
            StatusKind::MaxForcePoints { gained: true, by } => self.max_fp_gained += by,
            StatusKind::MaxForcePoints { gained: false, by } => self.max_fp_lost += by,
        }
    }

    /// Its rows in the game's words.
    fn rows(&self, ui: &mut Ui) -> Vec<(Row, Vec<u8>)> {
        let line = |ui: &Ui, strref: u32| {
            ui.assets
                .as_ref()
                .and_then(|assets| assets.text(Source::Campaign, strref))
                .unwrap_or(Vec::new())
        };
        let mut rows = Vec::new();
        if self.journal {
            rows.push((Row::Journal, line(ui, JOURNAL_ADDED)));
        }
        if self.credits != 0 {
            let strref = if self.credits > 0 {
                CREDITS_RECEIVED
            } else {
                CREDITS_LOST
            };
            let amount = self.credits.abs().to_string();
            rows.push((Row::Credits, filled(line(ui, strref), amount.as_bytes())));
        }
        if self.xp != 0 {
            let xp = self.xp.to_string();
            rows.push((Row::Xp, filled(line(ui, XP_RECEIVED), xp.as_bytes())));
        }
        if self.dark {
            rows.push((Row::DarkSide, line(ui, DARK_SIDE)));
        }
        if self.light {
            rows.push((Row::LightSide, line(ui, LIGHT_SIDE)));
        }
        if !self.received.is_empty() {
            // KOTOR 2's line names them, joined; K1's has no place for
            // them, so they follow it.
            let names = self
                .received
                .iter()
                .map(|template| {
                    ui.assets
                        .as_mut()
                        .and_then(|assets| assets.item(template))
                        .map_or_else(|| template.clone().into_bytes(), |look| look.name)
                })
                .collect::<Vec<_>>()
                .join(&b", "[..]);
            let text = line(ui, ITEMS_RECEIVED);
            let text = if text.windows(CUSTOM.len()).any(|at| at == CUSTOM) {
                filled(text, &names)
            } else {
                [text.as_slice(), b": ", &names].concat()
            };
            rows.push((Row::Received, text));
        }
        if self.lost {
            rows.push((Row::Lost, line(ui, ITEMS_LOST)));
        }
        for (row, strref, mask) in [
            (
                Row::InfluenceGained,
                INFLUENCE_GAINED,
                self.influence_gained,
            ),
            (Row::InfluenceLost, INFLUENCE_LOST, self.influence_lost),
        ] {
            let names: Vec<Vec<u8>> = (0..COMPANIONS.len())
                .filter(|npc| mask & (1 << npc) != 0)
                .map(|npc| line(ui, COMPANIONS[npc]))
                .collect();
            if !names.is_empty() {
                rows.push((row, filled(line(ui, strref), &names.join(&b", "[..]))));
            }
        }
        for (row, strref, count) in [
            (Row::MaxFpGained, MAX_FP_GAINED, self.max_fp_gained),
            (Row::MaxFpLost, MAX_FP_LOST, self.max_fp_lost),
        ] {
            if count != 0 {
                rows.push((row, line(ui, strref)));
            }
        }
        rows
    }
}

pub fn marks(
    presented: Res<PresentedSnapshot>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    (replies, time): (Option<Res<crate::dialogue_view::ReplyInput>>, Res<Time>),
    mut seen: Local<Seen>,
    ui: Option<ResMut<Ui>>,
    (cues, mut told): (Option<ResMut<crate::sound::Cues>>, ResMut<Told>),
) {
    let (Some(own), Some(mut ui)) = (own, ui) else {
        return;
    };
    let (ui, seen) = (&mut *ui, &mut *seen);
    let now = time.elapsed_secs_f64();
    let snapshot = presented.0.current();
    let progress = &snapshot.progress;
    let library = swap.as_ref().and_then(|swap| swap.level.scripts());
    let module = snapshot.place.as_ref().map(|place| place.module.clone());
    let went_on = seen
        .at
        .as_ref()
        .is_some_and(|(was, tick)| *was == module && snapshot.tick >= *tick);
    seen.at = Some((module, snapshot.tick));
    // The statuses not seen yet: on a first look (a module's clock starts
    // over), every one there, raised in its first steps.
    if !went_on {
        seen.journal = None;
        seen.status_ms = None;
    }
    for kind in told.statuses.drain(..) {
        seen.gathering.add(&kind);
    }
    for notice in told.notices.drain(..) {
        let view = SummaryView {
            rows: vec![(Row::Notice, notice.into_bytes())],
            slide: 0.0,
            alpha: 1.0,
        };
        seen.up.push((view, now));
    }
    let after = seen.status_ms;
    for status in progress.statuses.iter() {
        if after.is_none_or(|after| status.at_ms > after) {
            seen.gathering.add(&status.kind);
            seen.status_ms = seen.status_ms.max(Some(status.at_ms));
        }
    }
    if let (Some(before), Some(assets)) = (&seen.journal, ui.assets.as_ref()) {
        let line = |strref| {
            fmt_odyssey::code_page(&assets.text(Source::Campaign, strref).unwrap_or(Vec::new()))
        };
        let moved = progress
            .journal
            .iter()
            .filter(|quest| !before.contains(quest));
        let mut sound = None;
        for (tag, state) in moved {
            let quest = library.and_then(|library| library.quests().get(tag));
            let name = quest.map_or_else(|| tag.clone(), |quest| line(quest.name));
            ui.vm
                .feedback
                .push(format!("{} {name}", line(JOURNAL_UPDATED)));
            // One sound for the step's quests, the completion's over a new
            // entry's.
            let ended = quest
                .and_then(|quest| quest.entry(u32::try_from(*state).ok()?))
                .is_some_and(|entry| entry.end);
            if ended || sound.is_none() {
                sound = Some(if ended {
                    crate::ui_sounds::QUEST_COMPLETE
                } else {
                    crate::ui_sounds::QUEST_NEW
                });
            }
        }
        if let (Some(sound), Some(mut cues)) = (sound, cues) {
            cues.0.push(crate::ui_sounds::cue(&presented.0, sound));
        }
    }
    seen.journal = Some(progress.journal.clone());
    // Held back while a conversation, a fade or a screen is up.
    let held = replies.is_some_and(|replies| replies.active)
        || crate::fade_view::covered(&presented.0) > 0.0
        || ui.stack.is_open();
    seen.clear_since = match (held, seen.clear_since) {
        (true, _) => None,
        (false, since) => Some(since.unwrap_or(now)),
    };
    let settled = seen
        .clear_since
        .is_some_and(|since| now - since >= SETTLE_S);
    if settled && seen.gathering != Summary::default() {
        let rows = std::mem::take(&mut seen.gathering).rows(ui);
        seen.lit = rows.iter().filter_map(|(row, _)| row.hud()).collect();
        seen.lit_until = now + LIT_S;
        let view = SummaryView {
            rows,
            slide: 0.0,
            alpha: 1.0,
        };
        seen.up.push((view, now));
    }
    seen.up.retain(|(_, shown)| now - shown < SHOWN_S + OUT_S);
    ui.vm.summaries = seen
        .up
        .iter()
        .map(|(view, shown)| {
            let age = now - shown;
            SummaryView {
                slide: (age / IN_S).min(1.0) as f32,
                alpha: (1.0 - (age - SHOWN_S) / OUT_S).clamp(0.0, 1.0) as f32,
                ..view.clone()
            }
        })
        .collect();
    let level = swap
        .as_ref()
        .zip(crate::cast::levelling(snapshot, own.0))
        .map(|(swap, levelling)| crate::cast::worn_sheet(snapshot, levelling, &swap.level).level);
    let may_level = level
        .zip(library)
        .is_some_and(|(level, library)| library.classes().may_level(level, progress.xp));
    if let Some(hud) = ui.vm.hud.as_mut() {
        hud.level_up = may_level;
        hud.lit = if now < seen.lit_until {
            seen.lit.clone()
        } else {
            Vec::new()
        };
    }
}
