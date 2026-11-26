//! Computers' conversations on KOTOR's computer screen (render side only:
//! it reads the presented snapshots' conversations and goods):
//!
//! - while this player is in a conversation whose DLG is a computer's
//!   (`ConversationType` 1), the `computer` screen shows its line and
//!   replies (`<CUSTOMn>` filled in), and what the terminals' scripts
//!   read: the user's Computer Use and Repair totals, with what
//!   effects, items and feats add and the key ability's modifier
//!   (`GetSkillRank` on `GetPartyMemberByIndex(0)`, the character its
//!   player controls, which is also what both games' computer screens
//!   show; in co-op the body standing in the conversation, a guest's
//!   companion too: `Engine::skilled`), and the
//!   stack of spikes and of parts they find on it (`GetItemPossessedBy`
//!   tags `K_COMPUTER_SPIKE`, `K_REPAIR_PART`, `GetNumStackedItems`);
//!   `dialogue_view` keeps only the voice;
//! - each new line starts the screen's rows over;
//! - a camera feed (a line filmed by a placed camera, while it plays; one
//!   with no text until the next line) steps the screen aside;
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
        // The Debug Mode map's computer opens a page of its own.
        app.add_plugins(crate::debug_terminal::DebugTerminalPlugin);
        app.add_systems(
            Update,
            (
                show.before(crate::ui_view::Stepped),
                // Photo mode takes the screen's keys, its pointer and the pad.
                answer
                    .after(crate::ui_view::Stepped)
                    .run_if(crate::photo_mode::off),
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
    if feeds(entry, talk.until_ms) {
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
    // The skills' totals as `GetSkillRank` reads them, the numbers both
    // games' computer screens show (`sim::stealth::rank`), the user's: a
    // guest's companion slices with its own Computer Use, though the
    // scripts' PC is the host's character.
    let user = [talk.listener, talk.pc]
        .into_iter()
        .find(|id| *id != script_ncs::vm::ObjectId::INVALID);
    let first = user.and_then(|id| snapshot.get(sim::SimId(id.0)));
    let library = swap.level.scripts().map(|library| &**library);
    let tsl = swap.level.game() == sim::combat::passive::Game::Tsl;
    let rank = |skill| {
        first
            .and_then(|pc| Some((pc.sheet?, pc.fighter.as_ref()?)))
            .map_or(0, |(own, fighter)| {
                let now = sim::combat::effects::sheet_now(own, fighter, snapshot.time_ms);
                sim::stealth::rank(library, &own, &now, skill, tsl)
            })
    };
    let (spikes, parts) = stacks(assets, &snapshot.goods, first.map_or(0, |pc| pc.id.0));
    let computer = ComputerView {
        message,
        replies,
        computer_use: rank(skills::COMPUTER_USE),
        spikes,
        repair: rank(skills::REPAIR),
        parts,
        leave: snapshot.may_leave(seat, talk),
        leave_key: map.shown(KeyAction::LeaveTalk),
    };
    Some((computer, (talk.id, talk.entry, talk.shown_ms)))
}

/// Whether a computer's line is on its camera feed: a line filmed by a
/// placed camera (its own `CameraAngle` is 6, its `CameraID` names the
/// camera) shows the camera's view while it plays (both games' live feed
/// screen, `computercamera`, whose Enter cancels the feed back to the
/// computer), one with no text until the next line. Peragus' shift
/// console's camera lines carry their room's name and a 20 s delay. A
/// `CameraID` on a line of another angle films nothing: the cinematic
/// view does not use it either (`play_view::line_shot`). Some mods'
/// computers give every line `CameraID` -1, and their silent lines that
/// offer replies show those replies on the computer's screen.
fn feeds(entry: &fmt_odyssey::dlg::Node, until_ms: u64) -> bool {
    entry.camera_angle == u32::from(crate::play_view::ANGLE_PLACED)
        && (entry.is_silent() || until_ms != u64::MAX)
}

/// Whether this player's computer conversation is on a camera feed
/// (`feeds`).
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
                .is_some_and(|entry| feeds(entry, talk.until_ms))
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
    pad: Option<Res<crate::pad::PadPresses>>,
) {
    let computer = ui.vm.computer.as_ref();
    let offered = computer.map(|computer| computer.replies.len());
    let may_leave = computer.is_some_and(|computer| computer.leave == Some(true));
    let map = map.as_deref().unwrap_or(KeyMap::defaults());
    let leave_key = map.key(KeyAction::LeaveTalk);
    // The key that leaves is not held into play (`swallow`), as on the
    // cinematic screen.
    if let Some((keys, key)) = keys
        .as_deref()
        .zip(leave_key)
        .filter(|(keys, _)| may_leave && map.key_pressed(KeyAction::LeaveTalk, keys))
    {
        input.pick = Some(Pick::Leave);
        swallowed.key(key, keys);
    }
    // A camera feed has no screen: a click, Enter or a pad's A skips its
    // line (what the pad held is taken as the feed lets it go,
    // `pad::sample`).
    if ui.vm.camera_feed {
        let clicked = buttons
            .as_ref()
            .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
        let confirmed = keys
            .as_ref()
            .is_some_and(|keys| CONFIRM.iter().any(|code| keys.just_pressed(*code)))
            || pad.is_some_and(|pad| pad.any(&[bevy::input::gamepad::GamepadButton::South]));
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

/// The probes (`kjk sim probes --only "computer camera feeds"`): which of a
/// computer conversation's lines step its screen aside for a camera feed.
/// The DLG is written here as a GFF and read back. A silent line or a
/// timed line filmed by a placed camera (angle 6) is a feed. A silent line
/// with `CameraID` -1 and no angle of its own, or with a camera id but
/// angle 0, is not, so its replies show on the screen.
#[cfg(feature = "probes")]
pub fn feed_probes() -> sim::probe::Checks {
    use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};
    let field = |label: &str, value| GffField {
        label: label.as_bytes().to_vec(),
        value,
    };
    let entry = |text: &str, angle: u32, camera: i32, delay: u32| GffStruct {
        id: 0,
        fields: vec![
            field(
                "Text",
                GffValue::LocString(LocString {
                    strref: u32::MAX,
                    strings: (!text.is_empty())
                        .then(|| (0, text.as_bytes().to_vec()))
                        .into_iter()
                        .collect(),
                }),
            ),
            field("CameraAngle", GffValue::Dword(angle)),
            field("CameraID", GffValue::Int(camera)),
            field("Delay", GffValue::Dword(delay)),
        ],
    };
    const NONE: u32 = u32::MAX;
    // (name, line, the line's end in ms or none, a feed)
    let cases = [
        (
            "silent_placed_camera_feeds",
            entry("", 6, 8, 20),
            20_000,
            true,
        ),
        (
            "silent_placed_camera_untimed_feeds",
            entry("", 6, 16, NONE),
            u64::MAX,
            true,
        ),
        (
            "timed_line_on_placed_camera_feeds",
            entry("Shift 2", 6, 3, 20),
            20_000,
            true,
        ),
        (
            "untimed_line_on_placed_camera_shows_screen",
            entry("ACCESS DENIED", 6, 5, NONE),
            u64::MAX,
            false,
        ),
        (
            "silent_line_camera_minus_one_shows_screen",
            entry("", 0, -1, NONE),
            u64::MAX,
            false,
        ),
        (
            "silent_line_camera_id_without_angle_shows_screen",
            entry("", 0, 4, NONE),
            u64::MAX,
            false,
        ),
    ];
    let root = GffStruct {
        id: 0,
        fields: vec![
            field(
                "EntryList",
                GffValue::List(cases.iter().map(|(_, line, ..)| line.clone()).collect()),
            ),
            field("ConversationType", GffValue::Int(1)),
        ],
    };
    let bytes = Gff {
        file_type: *b"DLG ",
        root,
    }
    .write();
    let dlg = fmt_odyssey::dlg::Dlg::parse(&bytes);
    cases
        .iter()
        .enumerate()
        .map(|(i, (name, _, until_ms, expected))| {
            let result = match dlg.as_ref().ok().and_then(|dlg| dlg.entries.get(i)) {
                None => Err("the DLG made here did not read back".to_string()),
                Some(line) => {
                    let feeds = feeds(line, *until_ms);
                    sim::probe::check(
                        feeds == *expected,
                        format!(
                            "angle {} camera {} silent {}: a feed {feeds}, expected {expected}",
                            line.camera_angle,
                            line.camera_id,
                            line.is_silent()
                        ),
                    )
                }
            };
            (*name, result)
        })
        .collect()
}
