//! The co-op lobby (`vm::LobbyView`): the players who gathered, the save
//! about to be played, the saber rules and the collision, and the
//! companions each guest chooses before the host starts the game.
//!
//! KOTOR 2 has no lobby, so the screen is assembled from the party
//! selection layout (`partyselect_p`), moved about in `shape`. Every piece
//! is KOTOR 2's own art:
//! - the title, the bars and the Back and Done buttons are the layout's
//!   own, moved to the bottom row;
//! - the Players, Game and description panels are copies of the panel
//!   behind the companion's name (`LBL_NAMEBACK`);
//! - each player slot's portrait is a copy of a companion portrait
//!   (`LBL_CHAR0`), and its frame a copy of a companion button
//!   (`BTN_NPC0`), drawn in the highlight colour round the viewer's own
//!   slot;
//! - the saber rules and collision rows, Change Save and Invite Friends are
//!   copies of the Add button (`BTN_ACCEPT`) and the Back button;
//! - the companions keep the layout's portraits and buttons (`BTN_NPCn`,
//!   `LBL_CHARn`), set in one row in the order of the lobby's companions,
//!   each with its name and who has it under it in KOTOR 2's small
//!   dialogue font. A companion the story took away shows the layout's
//!   "no companion" art (`LBL_NAn`) in place of the portrait.
//!
//! The pointer over a control selects it, as on KOTOR 2's menus, and the
//! description strip says what choosing it does. The keyboard and the
//! gamepad reach every control by the nearest one in each direction.
//!
//! The host chooses the saber rules, the collision, and which companions
//! the game controls in the places no guest took, and starts the game. A
//! guest chooses a companion (one another guest plays swaps the two) and
//! marks Ready. Each press is an `Intent::Lobby`: the host's game decides.

use fmt_odyssey::gui::{Border, Control, Gui, Rect};

use crate::draw::{DrawList, Quad};
use crate::intent::{Intent, LobbyIntent};
use crate::paint::{ControlState, Painter, rect_of};
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{hidden, image, text};
use crate::strings;
use crate::vm::{
    LobbyCompanionState, LobbyCompanionView, LobbySlotState, LobbySlotView, LobbyView, ViewModel,
};

/// The most companions the row shows: ten, the most either game has
/// recruited at once (KOTOR 1 has nine; of KOTOR 2's twelve, Hanharr and
/// Mira never travel together, nor the Disciple and the Handmaiden). The
/// layout's last two places are hidden.
const COMPANIONS: usize = 10;
/// The layout's companion places.
const PLACES: usize = 12;
/// The player slots.
const SLOTS: usize = 3;
/// KOTOR 2's small dialogue font, for the rows under the portraits and the
/// panels' text.
const SMALL: &str = "dialogfont10x10";
/// Text aligned to the top left, and to the top and centred across.
const TOP_LEFT: i32 = 1;
const TOP_CENTRED: i32 = 2;

/// Where each piece goes on the layout's 800 by 600 frame.
const PANEL_PLAYERS: [i32; 4] = [78, 155, 322, 197];
const PANEL_GAME: [i32; 4] = [410, 155, 312, 197];
const PANEL_DESC: [i32; 4] = [78, 452, 644, 50];
const HEADS: [(&str, &str, [i32; 4]); 3] = [
    (
        "LBL_HEAD_PLAYERS",
        strings::LOBBY_PLAYERS,
        [82, 136, 300, 18],
    ),
    ("LBL_HEAD_GAME", strings::LOBBY_GAME, [414, 136, 300, 18]),
    (
        "LBL_HEAD_COMPANIONS",
        strings::LOBBY_COMPANIONS,
        [82, 354, 300, 18],
    ),
];
/// The player slots: the first's top, the pitch between them, and the side
/// of their portraits. They are the screen's main part: larger than the
/// companions' portraits.
const SLOT_TOP: i32 = 160;
const SLOT_PITCH: i32 = 64;
const SLOT_PORTRAIT: i32 = 56;
/// The companions' row: its left, the pitch between portraits, the top of
/// the portraits, and their side.
const ROW_LEFT: i32 = 78;
const ROW_PITCH: i32 = 60;
const ROW_TOP: i32 = 372;
const ROW_PORTRAIT: i32 = 38;
/// The bottom row's buttons: tag, left, width.
const BUTTONS: [(&str, i32, i32); 4] = [
    ("BTN_BACK", 75, 120),
    ("BTN_INVITE", 205, 160),
    ("BTN_ACCEPT", 375, 160),
    ("BTN_DONE", 565, 160),
];
/// The layout's pieces the lobby does not show.
const UNUSED: [&str; 8] = [
    "LBL_NAMEBACK",
    "LBL_3D",
    "LBL_COUNT",
    "LBL_BEVEL_M",
    "LBL_BEVEL_L",
    "LBL_NPC_NAME",
    "LBL_NPC_LEVEL",
    "LBL_BAR5",
];

/// A piece `shape` copies: the control it copies, its tag, where it goes,
/// and its font and alignment.
type Copied = (&'static str, String, [i32; 4], Option<(&'static str, i32)>);

#[derive(Default)]
pub struct Lobby {
    /// The control under the cursor, which the description strip describes.
    focus: Option<String>,
}

fn rect([left, top, width, height]: [i32; 4]) -> Rect {
    Rect {
        left,
        top,
        width,
        height,
    }
}

/// A copy of `gui`'s control `from`, tagged `tag`, at `at`, linked to
/// nothing (the nearest control each way is reached), with its text in
/// `font` and `alignment` when given. Panels go first, behind the rest.
/// Nothing without `from`.
fn copy(
    gui: &mut Gui,
    from: &str,
    tag: &str,
    at: [i32; 4],
    font: Option<(&str, i32)>,
    behind: bool,
) {
    let Some(original) = gui.find(from).cloned() else {
        return;
    };
    let id = gui
        .controls
        .iter()
        .map(|control| control.id)
        .max()
        .unwrap_or(0)
        + 1;
    let mut copied = Control {
        id,
        tag: tag.into(),
        extent: rect(at),
        moveto: None,
        ..original
    };
    if let (Some((font, alignment)), Some(words)) = (font, copied.text.as_mut()) {
        words.font = font.into();
        words.alignment = alignment;
        words.text.clear();
        words.strref = None;
    }
    match behind {
        true => gui.controls.insert(0, copied),
        false => gui.controls.push(copied),
    }
}

/// `gui`'s control `tag` moved to `at`.
fn place(gui: &mut Gui, tag: &str, at: [i32; 4]) {
    if let Some(control) = gui.controls.iter_mut().find(|control| control.tag == tag) {
        control.extent = rect(at);
    }
}

/// The index of a tag with `prefix` and a number after it.
fn index(tag: &str, prefix: &str) -> Option<usize> {
    tag.strip_prefix(prefix)?.parse().ok()
}

/// `parts` joined with new lines, in the game's code page.
fn lines(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.join(&b'\n')
}

/// A row of the Game panel: "Save: Endar Spire".
fn row(name: &str, value: &[u8]) -> Vec<u8> {
    let mut row = strings::bytes(&format!("{name}: "));
    row.extend_from_slice(value);
    row
}

/// A companion's name as a player reads it in the description strip.
fn name_of(companion: &LobbyCompanionView) -> String {
    fmt_odyssey::code_page(&companion.name)
}

/// The value after `now` in `values` (wrapping), and the name it is shown
/// by.
fn next<'a>(values: &[(&'a str, &'a str)], now: &str) -> Option<&'a str> {
    let at = values.iter().position(|(value, _)| *value == now);
    let next = at.map_or(0, |at| (at + 1) % values.len());
    values.get(next).map(|(value, _)| *value)
}

fn named<'a>(values: &[(&'a str, &'a str)], value: &str) -> &'a str {
    (values.iter())
        .find(|(of, _)| *of == value)
        .map_or("", |(_, name)| *name)
}

/// The saber rules the lobby offers: Movie Duels' only when it is
/// installed, as on the options page.
fn saber_rules(lobby: &LobbyView) -> Vec<(&'static str, &'static str)> {
    (super::options::SABER_RULES.iter())
        .filter(|(value, _)| lobby.movie_duels || *value == "ja")
        .copied()
        .collect()
}

impl Lobby {
    fn lobby(vm: &ViewModel) -> Option<&LobbyView> {
        vm.lobby.as_ref()
    }

    /// The companion in place `at` of the row.
    fn companion(vm: &ViewModel, at: usize) -> Option<&LobbyCompanionView> {
        Lobby::lobby(vm)?.companions.get(at)
    }

    /// The viewing player's slot.
    fn own(lobby: &LobbyView) -> Option<&LobbySlotView> {
        lobby.slots.get(lobby.own_slot)?.as_ref()
    }

    /// What choosing companion `companion` does for the viewing player.
    fn choose(lobby: &LobbyView, companion: &LobbyCompanionView) -> Option<LobbyIntent> {
        let npc = companion.npc;
        match (&companion.state, lobby.host) {
            (LobbyCompanionState::Unavailable, _) => None,
            (LobbyCompanionState::Free | LobbyCompanionState::WithParty, true) => {
                Some(LobbyIntent::Fill(npc))
            }
            (LobbyCompanionState::Played { .. }, true) => None,
            (LobbyCompanionState::Played { slot, .. }, false) if *slot == lobby.own_slot => None,
            (_, false) => Some(LobbyIntent::Choose(npc)),
        }
    }

    /// The label under a companion's portrait: who has them.
    fn label(companion: &LobbyCompanionView) -> Vec<u8> {
        match &companion.state {
            LobbyCompanionState::Free => strings::bytes(strings::LOBBY_FREE),
            LobbyCompanionState::Played { player, .. } => strings::bytes(player),
            LobbyCompanionState::WithParty => strings::bytes(strings::LOBBY_WITH_PARTY),
            LobbyCompanionState::Unavailable => strings::bytes(strings::LOBBY_NOT_AVAILABLE),
        }
    }

    /// What the description strip says about companion `companion`.
    fn told_of(lobby: &LobbyView, companion: &LobbyCompanionView) -> String {
        let name = name_of(companion);
        match (&companion.state, lobby.host) {
            (LobbyCompanionState::Unavailable, _) => strings::lobby_unavailable(&name),
            (LobbyCompanionState::Played { slot, player }, false) if *slot != lobby.own_slot => {
                strings::lobby_swap(&name, player)
            }
            (LobbyCompanionState::Played { player, .. }, _) => strings::lobby_played(&name, player),
            (_, true) => strings::lobby_fill(&name),
            (_, false) => strings::lobby_free(&name),
        }
    }

    /// The description strip.
    fn description(&self, vm: &ViewModel) -> ControlState {
        let (Some(lobby), Some(focus)) = (Lobby::lobby(vm), self.focus.as_deref()) else {
            return ControlState::default();
        };
        let words = match focus {
            "BTN_SABER" | "BTN_COLLISION" if !lobby.host => {
                strings::LOBBY_SETTINGS_GUEST.to_string()
            }
            "BTN_SABER" => super::key_mapping::with_bindings(strings::SABER_RULES_HELP, vm),
            "BTN_COLLISION" => strings::COLLISION_HELP.to_string(),
            tag => match index(tag, "BTN_NPC").and_then(|at| Lobby::companion(vm, at)) {
                Some(companion) => Lobby::told_of(lobby, companion),
                None => return ControlState::default(),
            },
        };
        text(words)
    }

    /// A player slot's lines: the player, the character, its class and
    /// level, and the player's state and delay.
    fn slot_lines(slot: Option<&LobbySlotView>) -> Vec<u8> {
        let Some(slot) = slot else {
            return lines(&[
                strings::bytes(strings::LOBBY_OPEN),
                strings::bytes(strings::LOBBY_WAITING),
            ]);
        };
        let state = match slot.state {
            LobbySlotState::Host => strings::LOBBY_HOST,
            LobbySlotState::Choosing => strings::LOBBY_CHOOSING,
            LobbySlotState::Ready => strings::LOBBY_READY,
            LobbySlotState::Connecting => strings::LOBBY_CONNECTING,
        };
        let state = match slot.delay_ms.filter(|ms| *ms > 0) {
            Some(ms) => format!("{state}, {ms} ms"),
            None => state.to_string(),
        };
        let mut parts = vec![strings::bytes(&slot.player), slot.character.clone()];
        if !slot.class_level.is_empty() {
            parts.push(slot.class_level.clone());
        }
        parts.push(strings::bytes(&state));
        lines(&parts)
    }

    /// The Game panel's rows.
    fn game_lines(lobby: &LobbyView) -> Vec<u8> {
        let Some(save) = &lobby.save else {
            return strings::bytes(strings::LOBBY_CHOOSING_SAVE);
        };
        let game = match save.game {
            Some(vfs::Game::Kotor2) => strings::GAME_KOTOR2,
            _ => strings::GAME_KOTOR,
        };
        lines(&[
            row(strings::LOBBY_SAVE, &save.name),
            row(strings::LOBBY_AREA, &save.area),
            row(strings::LOBBY_GAME_ROW, &strings::bytes(game)),
            row(strings::LOBBY_SAVED, &strings::bytes(&save.saved)),
            row(strings::LOBBY_PARTY, &save.party.join(&b", "[..])),
        ])
    }

    /// A setting's row: its value's name, greyed for a guest.
    fn setting(lobby: &LobbyView, words: String) -> ControlState {
        ControlState {
            greyed: !lobby.host,
            ..text(words)
        }
    }

    /// A companion place's pieces.
    fn companion_state(tag: &str, lobby: &LobbyView) -> Option<ControlState> {
        let piece = |prefix: &str| index(tag, prefix);
        let at = piece("BTN_NPC")
            .or_else(|| piece("LBL_CHAR"))
            .or_else(|| piece("LBL_NA"))
            .or_else(|| piece("LBL_CNAME"))?;
        let Some(companion) = lobby.companions.get(at).filter(|_| at < COMPANIONS) else {
            return Some(hidden());
        };
        // One the story took away shows KOTOR 2's own "no companion" art
        // (`LBL_NAn`, `uibit_no_npc_p`) in place of the portrait, as its
        // party selection screen shows a companion who is not available.
        let away = companion.state == LobbyCompanionState::Unavailable;
        Some(match () {
            () if piece("LBL_CHAR").is_some() && away => hidden(),
            () if piece("LBL_CHAR").is_some() => image(companion.portrait.clone()),
            () if piece("LBL_NA").is_some() => ControlState {
                hidden: !away,
                ..ControlState::default()
            },
            () if piece("LBL_CNAME").is_some() => {
                let first = companion
                    .name
                    .split(|byte| *byte == b' ')
                    .next()
                    .unwrap_or(&[]);
                ControlState {
                    shorter: vec![lines(&[first.to_vec(), Lobby::label(companion)])],
                    ..text_bytes(lines(&[companion.name.clone(), Lobby::label(companion)]))
                }
            }
            () => ControlState::default(),
        })
    }
}

/// A control showing `words`, already in the game's code page.
fn text_bytes(words: Vec<u8>) -> ControlState {
    ControlState {
        text: Some(words),
        ..ControlState::default()
    }
}

impl Screen for Lobby {
    fn id(&self) -> ScreenId {
        ScreenId("lobby")
    }

    fn layout(&self) -> &str {
        "partyselection"
    }

    fn shape(&self, gui: &mut Gui) {
        if gui.find("LBL_GAMEINFO").is_some() {
            return;
        }
        for at in [PANEL_PLAYERS, PANEL_GAME, PANEL_DESC] {
            let tag = format!("PNL_{}_{}", at[0], at[1]);
            copy(gui, "LBL_NAMEBACK", &tag, at, None, true);
        }
        let small = Some((SMALL, TOP_LEFT));
        let mut made: Vec<Copied> = (HEADS.iter())
            .map(|(tag, _, at)| ("LBL_NPC_NAME", tag.to_string(), *at, small))
            .collect();
        for slot in 0..SLOTS as i32 {
            let top = SLOT_TOP + slot * SLOT_PITCH;
            made.extend([
                (
                    "LBL_NAMEBACK",
                    format!("BTN_SLOT{slot}"),
                    [82, top - 2, 314, SLOT_PITCH - 2],
                    None,
                ),
                (
                    "LBL_CHAR0",
                    format!("LBL_SLOTP{slot}"),
                    [86, top + 1, SLOT_PORTRAIT, SLOT_PORTRAIT],
                    None,
                ),
                (
                    "LBL_NPC_NAME",
                    format!("LBL_SLOT{slot}"),
                    [150, top + 9, 242, 44],
                    small,
                ),
            ]);
        }
        made.extend([
            (
                "LBL_NPC_NAME",
                "LBL_GAMEINFO".into(),
                [418, 160, 296, 110],
                small,
            ),
            ("BTN_ACCEPT", "BTN_SABER".into(), [418, 276, 296, 24], None),
            (
                "BTN_ACCEPT",
                "BTN_COLLISION".into(),
                [418, 304, 296, 24],
                None,
            ),
            (
                "LBL_NPC_NAME",
                "LBL_ADDRESS".into(),
                [418, 334, 296, 16],
                small,
            ),
            ("LBL_NPC_NAME", "LBL_DESC".into(), [86, 456, 628, 42], small),
            ("BTN_BACK", "BTN_INVITE".into(), [0, 516, 0, 28], None),
        ]);
        for at in 0..PLACES as i32 {
            let left = ROW_LEFT + at.min(COMPANIONS as i32 - 1) * ROW_PITCH;
            for prefix in ["BTN_NPC", "LBL_CHAR", "LBL_NA"] {
                let side = ROW_PORTRAIT;
                place(
                    gui,
                    &format!("{prefix}{at}"),
                    [left + (ROW_PITCH - side) / 2, ROW_TOP, side, side],
                );
            }
            // Wider than its column, so a long name stays on one line.
            let under = [left - 10, ROW_TOP + ROW_PORTRAIT + 2, ROW_PITCH + 20, 36];
            made.push((
                "LBL_NPC_NAME",
                format!("LBL_CNAME{at}"),
                under,
                Some((SMALL, TOP_CENTRED)),
            ));
        }
        for (from, tag, at, font) in made {
            copy(gui, from, &tag, at, font, false);
        }
        for (tag, left, width) in BUTTONS {
            place(gui, tag, [left, 516, width, 28]);
        }
        place(gui, "LBL_TITLE", [90, 57, 620, 24]);
        // Out of the way, so that no portrait keeps its shape about them
        // when the frame is stretched wide.
        for tag in UNUSED {
            place(gui, tag, [0, 0, 0, 0]);
        }
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        if tag == "LBL_TITLE" {
            return text(strings::LOBBY_TITLE);
        }
        let Some(lobby) = Lobby::lobby(vm) else {
            return hidden();
        };
        if let Some((_, words, _)) = HEADS.iter().find(|(head, ..)| *head == tag) {
            return text(*words);
        }
        if UNUSED.contains(&tag) || index(tag, "BTN_SLOT").is_some() {
            return hidden();
        }
        if let Some(slot) = index(tag, "LBL_SLOTP") {
            let shown = lobby.slots.get(slot).and_then(Option::as_ref);
            return image(shown.and_then(|slot| slot.portrait.clone()));
        }
        if let Some(slot) = index(tag, "LBL_SLOT") {
            return text_bytes(Lobby::slot_lines(
                lobby.slots.get(slot).and_then(Option::as_ref),
            ));
        }
        if let Some(state) = Lobby::companion_state(tag, lobby) {
            return state;
        }
        let ready = Lobby::own(lobby).is_some_and(|own| own.state == LobbySlotState::Ready);
        match tag {
            "LBL_GAMEINFO" => text_bytes(Lobby::game_lines(lobby)),
            "BTN_SABER" => Lobby::setting(
                lobby,
                format!(
                    "{}: {}",
                    strings::SABER_RULES,
                    named(super::options::SABER_RULES, &lobby.saber_rules)
                ),
            ),
            "BTN_COLLISION" => Lobby::setting(
                lobby,
                named(super::options::COLLISIONS, &lobby.collision).to_string(),
            ),
            "LBL_ADDRESS" if lobby.host => text(strings::lobby_address(&lobby.address)),
            "LBL_DESC" => self.description(vm),
            "BTN_BACK" => text(strings::LOBBY_LEAVE),
            "BTN_INVITE" if lobby.host && lobby.steam => text(strings::LOBBY_INVITE),
            "BTN_ACCEPT" if lobby.host => text(strings::LOBBY_CHANGE_SAVE),
            "BTN_DONE" if lobby.host => text(strings::LOBBY_START),
            "BTN_DONE" => ControlState {
                selected: ready,
                ..text(strings::LOBBY_READY)
            },
            "LBL_ADDRESS" | "BTN_INVITE" | "BTN_ACCEPT" => hidden(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(lobby) = Lobby::lobby(vm) else {
            return;
        };
        let host = lobby.host;
        let intent = match tag {
            "BTN_BACK" => Some(LobbyIntent::Leave),
            "BTN_DONE" if host => Some(LobbyIntent::Start),
            "BTN_DONE" => {
                let ready = Lobby::own(lobby).is_some_and(|own| own.state == LobbySlotState::Ready);
                Some(LobbyIntent::Ready(!ready))
            }
            "BTN_ACCEPT" if host => Some(LobbyIntent::ChangeSave),
            "BTN_INVITE" if host && lobby.steam => Some(LobbyIntent::Invite),
            "BTN_SABER" if host => next(&saber_rules(lobby), &lobby.saber_rules)
                .map(|rules| LobbyIntent::SaberRules(rules.into())),
            "BTN_COLLISION" if host => next(super::options::COLLISIONS, &lobby.collision)
                .map(|mode| LobbyIntent::Collision(mode.into())),
            tag => (index(tag, "BTN_NPC").and_then(|at| lobby.companions.get(at)))
                .and_then(|companion| Lobby::choose(lobby, companion)),
        };
        out.extend(intent.map(Intent::Lobby));
    }

    /// Back leaves the lobby, as Leave does.
    fn back(&mut self, out: &mut Vec<Intent>) {
        out.push(Intent::Lobby(LobbyIntent::Leave));
    }

    fn focused(&mut self, tag: &str, _vm: &ViewModel) {
        self.focus = Some(tag.to_string());
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_DONE")
    }

    /// The viewing player's own slot, framed in the highlight colour.
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let mut out = DrawList::default();
        let Some(lobby) = Lobby::lobby(vm) else {
            return out.quads;
        };
        let tag = format!("BTN_SLOT{}", lobby.own_slot);
        if let Some((frame, border)) = gui
            .find(&tag)
            .and_then(|frame| Some((frame, frame.border.as_ref()?)))
        {
            let color = painter.palette.hilight;
            let border = Border {
                color: Some(color),
                fill: String::new(),
                ..border.clone()
            };
            let [r, g, b] = color;
            painter.border(&border, rect_of(frame), [r, g, b, 1.0], &mut out);
        }
        out.quads
    }
}
