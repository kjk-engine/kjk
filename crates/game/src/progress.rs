//! The story's progress on the HUD (I19), from the presented snapshot's
//! (`sim::Progress`): the game's "Journal Updated:" with the quest's name in
//! the feedback log, with the journal's sound (`ui_sounds`); and KOTOR's
//! status summary (`ui::screens::status_summary`)
//! of what the scripts told the party (`Progress::statuses`).
//!
//! As KOTOR's client does, the statuses gather while a conversation, a fade
//! or a screen is up, and the summary shows a quarter second after none
//! is, the HUD's status icons lit with it for a few seconds.
//!
//! With the Interface option "Notifications in the corner" off, the summary
//! works as KOTOR 2's does. It stands in the middle of the window with an
//! OK button and stays until the player presses OK (`ui_view`: a click on
//! the OK, Enter, Escape, or a pad's A, B, X or Y). Statuses that come
//! while it is up gather into the next summary, which shows a quarter
//! second after the player closes this one. In a window, while no other
//! player has joined, the world waits while the summary is up
//! (`SummaryHold`, `sim_host`). A headless run, and a session other players
//! have joined, go on meanwhile.
//!
//! With the option on, the summaries slide in at the window's right side
//! instead, as kjk used to show them, each fading out on its own after 6
//! seconds, and a datapad the party picks up gets a box of its own there
//! for 8 seconds: its icon, its name and the start of its words, and the
//! journal key beside "Read datapad" (no key while a pad plays and binds
//! the journal to no button). KOTOR 2 has no such box, so with the option
//! off none shows. In both cases the journal key opens the inventory on its
//! Datapads filter with the datapad chosen (`ui_view::in_game`) for those 8
//! seconds, and the box goes.
//!
//! KOTOR 2's feedback option "Status Summary" (`ui::feedback`, on unless
//! the player turns it off) decides whether the summary in the middle shows
//! at all. With it off, no summary shows there and the world never waits
//! for one. The HUD's status icons still light, as KOTOR 2 flashes them
//! with the option off. The notifications in the corner are kjk's own and
//! do not wait for anything, so with "Notifications in the corner" on they
//! still slide in whatever the Status Summary option says
//! (`summary_box`).

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
/// How long a picked-up datapad's box stays (s), and how many of its
/// words' letters it shows before cutting them off.
const DATAPAD_S: f64 = 8.0;
const EXCERPT: usize = 150;
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

/// Whether a status summary waits for its OK (`ui_view` sets it): the
/// world waits with it (`sim_host`).
#[derive(Resource, Default)]
pub struct SummaryHold(pub bool);

/// What was last seen (where and when: a load or another module starts
/// over), the summary gathering, the HUD's icons lit, and the summaries up.
#[derive(Default)]
pub struct Seen {
    at: Option<(Option<String>, u64)>,
    journal: Option<Vec<(String, i32)>>,
    status_ms: Option<u64>,
    gathering: Summary,
    /// When the first status of the summary gathering came.
    gathered_since: Option<f64>,
    clear_since: Option<f64>,
    lit: Vec<&'static str>,
    lit_until: f64,
    up: Vec<Up>,
    /// The party's items last seen (by id), to tell a datapad picked up.
    carried: Option<std::collections::BTreeSet<u32>>,
}

/// A summary up: what it shows, when it came up, how long it stays (s),
/// the datapad it is about (its item id), if it is a datapad's box, and
/// whether it waits for its OK instead (then `stays` is not read).
struct Up {
    view: SummaryView,
    shown: f64,
    stays: f64,
    datapad: Option<u32>,
    waits: bool,
}

impl Up {
    fn new(view: SummaryView, shown: f64) -> Up {
        Up {
            view,
            shown,
            stays: SHOWN_S,
            datapad: None,
            waits: false,
        }
    }

    /// Whether it is still up at `now`: one waiting for its OK until the
    /// player presses it, the rest until they have faded out.
    fn up(&self, now: f64) -> bool {
        self.waits || now - self.shown < self.stays + OUT_S
    }
}

/// How a gathered summary shows. `corner` is kjk's Interface option
/// "Notifications in the corner", and `summary` is KOTOR 2's feedback option
/// "Status Summary". The answer is `Some(false)` when the summary slides in
/// at the corner and goes on its own, and `Some(true)` when it stands in the
/// middle and waits for its OK. It is `None` when it does not show at all:
/// the corner is off and so is the Status Summary option.
fn summary_box(corner: bool, summary: bool) -> Option<bool> {
    match (corner, summary) {
        (true, _) => Some(false),
        (false, true) => Some(true),
        (false, false) => None,
    }
}

/// Whether a gathered summary shows at `now`: a quarter second (`SETTLE_S`)
/// after its first status came (`gathered`) and nothing held it back
/// (`clear`), whichever is later.
fn settled(clear: Option<f64>, gathered: Option<f64>, now: f64) -> bool {
    match (clear, gathered) {
        (Some(clear), Some(gathered)) => now - clear.max(gathered) >= SETTLE_S,
        _ => false,
    }
}

/// The Status Summary option's probes (`kjk sim probes --only "status
/// summary option"`): with the corner off, the summary waits for its OK
/// while the option is on and does not show while it is off. With the
/// corner on, the summary slides in either way. An unset option is on, as
/// KOTOR 2's default is. A summary waits a quarter second from its first
/// status.
#[cfg(feature = "probes")]
pub fn summary_option_probes() -> sim::probe::Checks {
    use sim::probe::check;
    let shown = [(false, true), (false, false), (true, true), (true, false)]
        .map(|(corner, summary)| summary_box(corner, summary));
    let unset = ui::feedback::on(&Default::default(), ui::feedback::STATUS_SUMMARY);
    let off = ui::feedback::on(
        &[(
            ui::feedback::STATUS_SUMMARY.to_string(),
            "false".to_string(),
        )]
        .into(),
        ui::feedback::STATUS_SUMMARY,
    );
    vec![
        (
            "the summary waits with the option on and does not show with it off",
            check(
                shown == [Some(true), None, Some(false), Some(false)],
                format!("{shown:?}"),
            ),
        ),
        (
            "a summary waits a quarter second from its first status, not from the last hold",
            {
                // Clear for 10 s; statuses come at 10 s.
                let waits = [10.1, 10.3].map(|now| settled(Some(0.0), Some(10.0), now));
                check(waits == [false, true], format!("{waits:?}"))
            },
        ),
        (
            "the option is on until the player turns it off",
            check(unset && !off, format!("unset {unset}, false {off}")),
        ),
    ]
}

/// The start of a datapad's `words` on one line: up to `EXCERPT` letters,
/// cut at a space and followed by "..." when there are more.
fn excerpt(words: &[u8]) -> Vec<u8> {
    let flat: Vec<u8> = words
        .iter()
        .map(|&byte| {
            if byte == b'\n' || byte == b'\r' {
                b' '
            } else {
                byte
            }
        })
        .collect();
    let flat = flat.trim_ascii();
    if flat.len() <= EXCERPT {
        return flat.to_vec();
    }
    let cut = flat[..EXCERPT]
        .iter()
        .rposition(|&byte| byte == b' ')
        .unwrap_or(EXCERPT);
    [flat[..cut].trim_ascii_end(), b"..."].concat()
}

/// The datapads among the party's items new since last seen (item id and
/// template), and the party's items now.
fn new_datapads(
    snapshot: &sim::Snapshot,
    before: Option<&std::collections::BTreeSet<u32>>,
    ui: &mut Ui,
) -> (Vec<(u32, String)>, std::collections::BTreeSet<u32>) {
    let carried: Vec<(u32, String)> = snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .map(|(id, item)| (id, item.template.clone()))
        .collect();
    let picked = before.map_or(Vec::new(), |before| {
        carried
            .iter()
            .filter(|(id, _)| !before.contains(id))
            .filter(|(_, template)| {
                ui.assets
                    .as_mut()
                    .and_then(|assets| assets.item(template))
                    .is_some_and(|look| look.kind == ui::vm::ItemKind::Datapad)
            })
            .cloned()
            .collect()
    });
    (picked, carried.into_iter().map(|(id, _)| id).collect())
}

/// A picked-up datapad's box: its icon, and its name over the start of
/// its words.
fn datapad_box(ui: &mut Ui, template: &str) -> SummaryView {
    let look = ui.assets.as_mut().and_then(|assets| assets.item(template));
    let (name, icon, words) = look.map_or_else(
        || (template.as_bytes().to_vec(), None, Vec::new()),
        |look| (look.name, Some(look.icon), excerpt(&look.description)),
    );
    let mut text = name;
    if !words.is_empty() {
        text.push(b'\n');
        text.extend(words);
    }
    SummaryView {
        rows: vec![(Row::Datapad, text)],
        icon,
        ..SummaryView::default()
    }
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

pub(crate) fn marks(
    presented: Res<PresentedSnapshot>,
    (own, swap): (Option<Res<OwnSeat>>, Option<Res<SceneSwap>>),
    (replies, time): (Option<Res<crate::dialogue_view::ReplyInput>>, Res<Time>),
    mut seen: Local<Seen>,
    ui: Option<ResMut<Ui>>,
    (cues, mut told): (Option<ResMut<crate::sound::Cues>>, ResMut<Told>),
    prompts: crate::pad::Prompts,
) {
    let (Some(_), Some(mut ui)) = (own, ui) else {
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
        seen.carried = None;
    }
    for kind in told.statuses.drain(..) {
        seen.gathering.add(&kind);
    }
    for notice in told.notices.drain(..) {
        let view = SummaryView {
            rows: vec![(Row::Notice, notice.into_bytes())],
            slide: 0.0,
            alpha: 1.0,
            ..SummaryView::default()
        };
        seen.up.push(Up::new(view, now));
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
    // The player pressed the OK of the summary waiting for it
    // (`ui_view`).
    if std::mem::take(&mut ui.summary_closed) {
        seen.up.retain(|up| !up.waits);
    }
    let corner = ui::draw::kjk_option(&ui.vm.settings, ui::draw::CORNER_NOTICES);
    let summary = ui::feedback::on(&ui.vm.settings, ui::feedback::STATUS_SUMMARY);
    let shown = summary_box(corner, summary);
    // A summary waiting for its OK goes as the notifications move to the
    // corner, or as the Status Summary option is turned off, as if the
    // player had pressed it.
    if shown != Some(true) {
        seen.up.retain(|up| !up.waits);
    }
    // Held back while a conversation, a fade or a screen is up, and while
    // a summary waits for its OK.
    let held = replies.is_some_and(|replies| replies.active)
        || crate::fade_view::covered(&presented.0) > 0.0
        || ui.stack.is_open()
        || seen.up.iter().any(|up| up.waits);
    seen.clear_since = match (held, seen.clear_since) {
        (true, _) => None,
        (false, since) => Some(since.unwrap_or(now)),
    };
    // KOTOR 2 starts its quarter second when the first status comes, so
    // statuses raised a few frames apart (a quest's journal entry and its
    // experience) show in one summary.
    if seen.gathering == Summary::default() {
        seen.gathered_since = None;
    } else {
        seen.gathered_since.get_or_insert(now);
    }
    if settled(seen.clear_since, seen.gathered_since, now) {
        seen.gathered_since = None;
        let rows = std::mem::take(&mut seen.gathering).rows(ui);
        // The HUD's status icons light whether the summary shows or not, as
        // KOTOR 2 flashes them with its Status Summary option off.
        seen.lit = rows.iter().filter_map(|(row, _)| row.hud()).collect();
        seen.lit_until = now + LIT_S;
        if let Some(waits) = shown {
            // KOTOR 2's waits for its OK: it shows whole at once.
            let view = SummaryView {
                rows,
                slide: if waits { 1.0 } else { 0.0 },
                alpha: 1.0,
                ok: waits.then_some(false),
                ..SummaryView::default()
            };
            seen.up.push(Up {
                waits,
                ..Up::new(view, now)
            });
        }
    }
    // A datapad picked up: a box of its own at once, over a conversation
    // or a screen too (the loot screen it was taken from).
    let (picked, carried) = new_datapads(snapshot, seen.carried.as_ref(), ui);
    seen.carried = Some(carried);
    for (item, template) in picked {
        seen.up.push(Up {
            stays: DATAPAD_S,
            datapad: Some(item),
            ..Up::new(datapad_box(ui, &template), now)
        });
    }
    // The journal key opened the inventory on a datapad
    // (`ui_view::in_game`): its box goes.
    if let Some(item) = ui.datapad_read.take() {
        seen.up.retain(|up| up.datapad != Some(item));
    }
    seen.up.retain(|up| up.up(now));
    // The newest datapad's box still fully up is the one the key reads.
    ui.datapad_up = seen
        .up
        .iter()
        .rev()
        .find(|up| up.datapad.is_some() && now - up.shown < up.stays)
        .and_then(|up| up.datapad);
    let prompt = prompts
        .bound_label(crate::settings::Action::Journal)
        .map(|key| (key, ui::strings::bytes(ui::strings::READ_DATAPAD)));
    // A datapad's box is kjk's own: only the notifications in the corner
    // show it.
    let hover = ui.summary_hover;
    ui.vm.summaries = seen
        .up
        .iter()
        .filter(|up| corner || up.datapad.is_none())
        .map(|up| {
            let age = now - up.shown;
            if up.waits {
                return SummaryView {
                    ok: Some(hover),
                    ..up.view.clone()
                };
            }
            SummaryView {
                slide: (age / IN_S).min(1.0) as f32,
                alpha: (1.0 - (age - up.stays) / OUT_S).clamp(0.0, 1.0) as f32,
                prompt: up.datapad.as_ref().and(prompt.clone()),
                ..up.view.clone()
            }
        })
        .collect();
    if let Some(hud) = ui.vm.hud.as_mut() {
        hud.lit = if now < seen.lit_until {
            seen.lit.clone()
        } else {
            Vec::new()
        };
    }
}
