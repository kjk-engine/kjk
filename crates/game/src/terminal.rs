//! Computers' conversations on KOTOR's computer screen (render side only:
//! it reads the presented snapshots' conversations and goods):
//!
//! - while this player is in a conversation whose DLG is a computer's
//!   (`ConversationType` 1), the `computer` screen shows its line and
//!   replies (`<CUSTOMn>` filled in), and what the terminals' scripts
//!   read: the first seat's Computer Use and Repair ranks as its effects
//!   leave them (`GetSkillRank` on `GetPartyMemberByIndex(0)`) and the
//!   stack of spikes and of parts they find on it (`GetItemPossessedBy`
//!   tags `K_COMPUTER_SPIKE`, `K_REPAIR_PART`, `GetNumStackedItems`);
//!   `dialogue_view` keeps only the voice;
//! - each new line starts the screen's rows over;
//! - a camera feed (a line with no text and a camera) steps the screen
//!   aside until the next line;
//! - while a camera feed shows, the feed's look is over the view
//!   (`ui::screens::feed`), and a click or Enter skips its line;
//! - it closes when the conversation ends, the screens over it staying;
//!   one closed any other way opens again;
//! - the screen's answers go into the command as the cinematic screen's do
//!   (`dialogue_view::ReplyInput`), a reply only among those offered;
//!   Leave too, its row or its key, while the conversation may be left
//!   (R6, `Snapshot::may_leave`).

use bevy::prelude::*;
use fmt_odyssey::dlg::Kind as DlgKind;
use sim::combat::sheet::skills;
use sim::inventory::{Goods, PARTY};
use sim::script::dialogue::Pick;
use ui::assets::GuiAssets;
use ui::draw::Textures;
use ui::intent::Intent;
use ui::skin::Source;
use ui::vm::ComputerView;

use crate::dialogue_view::{CONFIRM, ReplyInput, Watching, node_text, shown};
use crate::settings::{Action as KeyAction, KeyMap};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::{Ui, UiIntents};

const SCREEN: &str = "computer";
const SPIKE: &str = "k_computer_spike";
const PART: &str = "k_repair_part";

pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                show.before(crate::ui_view::Stepped),
                answer.after(crate::ui_view::Stepped),
            ),
        );
    }
}

/// The line shown: the conversation, its entry, when it was shown.
type Line = (u32, u32, u64);

/// The stacks of spikes and of parts the scripts find on `pc`: the first
/// item of each tag, by id, that it wears or the party carries.
fn stacks(assets: &mut GuiAssets, goods: &Goods, pc: u32) -> (u32, u32) {
    let (mut spikes, mut parts) = (None, None);
    let mut items: Vec<_> = goods
        .items
        .iter()
        .filter(|(_, item)| item.holder == pc || (item.holder == PARTY && item.slot.is_none()))
        .collect();
    items.sort_by_key(|(id, _)| *id);
    for (_, item) in items {
        let found = match assets
            .item_ref(&item.template)
            .map(|look| look.tag.as_str())
        {
            Some(SPIKE) => &mut spikes,
            Some(PART) => &mut parts,
            _ => continue,
        };
        found.get_or_insert(u32::from(item.stack));
    }
    (spikes.unwrap_or(0), parts.unwrap_or(0))
}

/// This player's computer conversation as the screen shows it, and its
/// line; none when it is in none, or its line is a camera feed.
fn view(
    assets: &mut GuiAssets,
    snapshot: &sim::Snapshot,
    (seat, followed): (sim::Seat, Option<sim::SimId>),
    swap: &SceneSwap,
    map: &KeyMap,
) -> Option<(ComputerView, Line)> {
    let talk = shown(snapshot, seat, followed)?;
    let dlg = swap.level.scripts()?.dialogue(&talk.dialog)?;
    if dlg.kind != DlgKind::Computer {
        return None;
    }
    let entry = dlg.entries.get(talk.entry as usize)?;
    if entry.is_silent() && entry.camera_id != 0 {
        return None;
    }
    let words = |node: Option<&fmt_odyssey::dlg::Node>| {
        node.map_or_else(Vec::new, |node| {
            node_text(
                node,
                &|strref| assets.text(Source::Campaign, strref),
                crate::dialogue_view::Fill::to(snapshot, talk.pc),
            )
        })
    };
    let message = words(Some(entry));
    let replies = if talk.auto {
        Vec::new()
    } else {
        talk.replies
            .iter()
            .map(|reply| words(dlg.replies.get(*reply as usize)))
            .collect()
    };
    let first = snapshot.own(sim::Seat(0));
    let ranks = first
        .and_then(|pc| Some((pc.sheet?, pc.fighter.as_ref()?)))
        .map_or([0; 8], |(sheet, fighter)| {
            sim::combat::effects::sheet_now(sheet, fighter, snapshot.time_ms).skills
        });
    let (spikes, parts) = stacks(assets, &snapshot.goods, first.map_or(0, |pc| pc.id.0));
    let computer = ComputerView {
        message,
        replies,
        computer_use: i32::from(ranks[skills::COMPUTER_USE]),
        spikes,
        repair: i32::from(ranks[skills::REPAIR]),
        parts,
        leave: snapshot.may_leave(seat, talk),
        leave_key: map.shown(KeyAction::LeaveTalk),
    };
    Some((computer, (talk.id, talk.entry, talk.shown_ms)))
}

/// Whether this player's computer conversation is on a camera feed (a line
/// with no text and a camera).
fn on_feed(
    snapshot: &sim::Snapshot,
    (seat, followed): (sim::Seat, Option<sim::SimId>),
    swap: &SceneSwap,
) -> bool {
    let Some(talk) = shown(snapshot, seat, followed) else {
        return false;
    };
    let dlg = swap
        .level
        .scripts()
        .and_then(|library| library.dialogue(&talk.dialog));
    dlg.is_some_and(|dlg| {
        dlg.kind == DlgKind::Computer
            && dlg
                .entries
                .get(talk.entry as usize)
                .is_some_and(|entry| entry.is_silent() && entry.camera_id != 0)
    })
}

fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    watching: Option<Res<Watching>>,
    map: Option<Res<KeyMap>>,
    swap: Option<Res<SceneSwap>>,
    mut ui: ResMut<Ui>,
    mut line: Local<Option<Line>>,
) {
    let ui = &mut *ui;
    let map = map.as_deref().unwrap_or(KeyMap::defaults());
    ui.vm.camera_feed = match (&presented, &own, &swap) {
        (Some(presented), Some(own), Some(swap)) => on_feed(
            presented.0.current(),
            (own.0, watching.as_ref().and_then(|watching| watching.0)),
            swap,
        ),
        _ => false,
    };
    let shown = match (presented, own, swap, ui.assets.as_mut()) {
        (Some(presented), Some(own), Some(swap), Some(assets)) => view(
            assets,
            presented.0.current(),
            (own.0, watching.and_then(|watching| watching.0)),
            &swap,
            map,
        ),
        _ => None,
    };
    let now = shown.as_ref().map(|(_, line)| *line);
    if now.is_some() && now != *line {
        ui.stack.rewind(SCREEN);
    }
    *line = now;
    ui.vm.computer = shown.map(|(computer, _)| computer);
    ui.stack.sync(SCREEN, ui.vm.computer.is_some(), &ui.vm);
}

/// The keys, the key map and the mouse buttons the answers read.
type AnswerInput<'a> = (
    Option<Res<'a, ButtonInput<KeyCode>>>,
    Option<Res<'a, KeyMap>>,
    Option<Res<'a, ButtonInput<MouseButton>>>,
);

/// The screen's answers, for the command: a reply among those offered, the
/// end of the line, or Leave (its row, or its key while the conversation
/// may be left).
fn answer(
    ui: Res<Ui>,
    mut intents: ResMut<UiIntents>,
    mut input: ResMut<ReplyInput>,
    (keys, map, buttons): AnswerInput,
    mut swallowed: ResMut<crate::swallow::Swallowed>,
) {
    let computer = ui.vm.computer.as_ref();
    let offered = computer.map(|computer| computer.replies.len());
    let may_leave = computer.is_some_and(|computer| computer.leave == Some(true));
    let leave_key = map
        .as_deref()
        .unwrap_or(KeyMap::defaults())
        .key(KeyAction::LeaveTalk);
    // The key that leaves is not held into play (`swallow`), as on the
    // cinematic screen.
    if let Some(keys) = keys
        .as_deref()
        .filter(|keys| may_leave && keys.just_pressed(leave_key))
    {
        input.pick = Some(Pick::Leave);
        swallowed.key(leave_key, keys);
    }
    // A camera feed has no screen: a click or Enter skips its line.
    if ui.vm.camera_feed {
        let clicked = buttons
            .as_ref()
            .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
        let confirmed = keys
            .as_ref()
            .is_some_and(|keys| CONFIRM.iter().any(|code| keys.just_pressed(*code)));
        if clicked || confirmed {
            input.pick = Some(Pick::Skip);
            if let Some(buttons) = buttons.as_deref().filter(|_| clicked) {
                swallowed.button(MouseButton::Left, buttons);
            }
            for code in CONFIRM {
                if let Some(keys) = keys.as_deref().filter(|keys| keys.just_pressed(code)) {
                    swallowed.key(code, keys);
                }
            }
        }
    }
    intents.0.retain(|intent| {
        match (intent, offered) {
            (Intent::Answer(Some(nth)), Some(offered)) if usize::from(*nth) < offered => {
                input.pick = Some(Pick::Reply(*nth));
            }
            (Intent::Answer(None), Some(_)) => input.pick = Some(Pick::Skip),
            (Intent::Leave, _) if may_leave => input.pick = Some(Pick::Leave),
            (Intent::Answer(_) | Intent::Leave, _) => {}
            _ => return true,
        }
        false
    });
}
