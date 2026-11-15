//! The co-op lobby screen (`screens::lobby`) over a sample lobby, in the
//! host's view and in a guest's: the words it shows, what each press asks
//! for, the description strip, and the screen built on a layout holding
//! the party selection's tags.

use super::menus::root;
use super::paint::{Fake, button};
use super::vm_screens::opened;
use super::{Check, check};
use crate::intent::{Intent, LobbyIntent};
use crate::paint::ControlState;
use crate::screen::Screen;
use crate::screens::lobby::Lobby;
use crate::vm::{
    LobbyCompanionState, LobbyCompanionView, LobbySaveView, LobbySlotState, LobbySlotView,
    LobbyView, ViewModel,
};

/// Carth played by Mia (slot 1), Bastila by Kalle (slot 2), Mission free,
/// and Zaalbar taken away by the story. Seen by the player in `own_slot`,
/// with Movie Duels when `movie_duels`.
fn sample(own_slot: usize, movie_duels: bool) -> ViewModel {
    let companion = |npc: u8, name: &str, state: LobbyCompanionState| LobbyCompanionView {
        npc,
        name: name.as_bytes().to_vec(),
        portrait: Some(format!("po_p{npc}")),
        state,
    };
    let played = |slot: usize, player: &str| LobbyCompanionState::Played {
        slot,
        player: player.into(),
    };
    let slot = |player: &str, character: &str, state: LobbySlotState, delay: Option<u16>| {
        Some(LobbySlotView {
            player: player.into(),
            character: character.as_bytes().to_vec(),
            class_level: b"Soldier, level 4".to_vec(),
            portrait: Some("po_pmhc01".into()),
            droid: false,
            state,
            delay_ms: delay,
        })
    };
    ViewModel {
        lobby: Some(LobbyView {
            host: own_slot == 0,
            own_slot,
            save: Some(LobbySaveView {
                name: b"Endar Spire".to_vec(),
                area: b"Command Module".to_vec(),
                game: Some(vfs::Game::Kotor1),
                saved: "2026-11-14".into(),
                party: vec![b"Dax".to_vec(), b"Carth Onasi".to_vec()],
            }),
            saber_rules: "ja".into(),
            collision: "limited".into(),
            companions: vec![
                companion(0, "Carth Onasi", played(1, "Mia")),
                companion(1, "Bastila Shan", played(2, "Kalle")),
                companion(2, "Mission Vao", LobbyCompanionState::Free),
                companion(3, "Zaalbar", LobbyCompanionState::Unavailable),
            ],
            slots: [
                slot("host", "Dax", LobbySlotState::Host, None),
                slot("Mia", "Carth Onasi", LobbySlotState::Choosing, Some(32)),
                slot("Kalle", "Bastila Shan", LobbySlotState::Ready, Some(48)),
            ],
            address: "192.168.1.20 : 7777".into(),
            steam: true,
            movie_duels,
        }),
        ..ViewModel::default()
    }
}

/// The sample with no companions at all (a new game): both guests fly
/// droids.
fn no_companions() -> ViewModel {
    let mut vm = sample(1, false);
    if let Some(lobby) = vm.lobby.as_mut() {
        lobby.companions.clear();
        for slot in lobby.slots.iter_mut().skip(1).flatten() {
            slot.character = crate::strings::bytes(crate::strings::LOBBY_DROID);
            slot.class_level.clear();
            slot.portrait = None;
            slot.droid = true;
        }
    }
    vm
}

/// The state of `tag` on `screen`, with the cursor on `focus` first.
fn state(screen: &mut Lobby, vm: &ViewModel, focus: Option<&str>, tag: &str) -> ControlState {
    if let Some(focus) = focus {
        screen.focused(focus, vm);
    }
    screen.state(&button(tag, [0, 0, 10, 10], b""), vm)
}

/// The words `tag` shows.
fn words(vm: &ViewModel, focus: Option<&str>, tag: &str) -> String {
    let shown = state(&mut Lobby::default(), vm, focus, tag);
    String::from_utf8_lossy(&shown.text.unwrap_or(Vec::new())).into_owned()
}

/// What pressing `tag` asks for.
fn pressed(vm: &ViewModel, tag: &str) -> Vec<Intent> {
    let mut out = Vec::new();
    Lobby::default().press(tag, vm, &mut out);
    out
}

fn lobby(intent: LobbyIntent) -> Vec<Intent> {
    vec![Intent::Lobby(intent)]
}

/// The views a check runs over: the host's and Kalle's.
const VIEWS: [(&str, usize); 2] = [("host", 0), ("guest", 2)];

/// Every row and label for each view: the panel heads, the Game rows, the
/// players' states and delays, who has each companion, and the buttons the
/// view offers.
fn words_shown() -> Vec<Check> {
    VIEWS
        .iter()
        .map(|&(view, own)| {
            let vm = sample(own, false);
            let host = own == 0;
            let wanted: Vec<(&str, &str)> = vec![
                ("LBL_TITLE", "Co-op Lobby"),
                ("LBL_HEAD_PLAYERS", "Players"),
                ("LBL_HEAD_GAME", "Game"),
                ("LBL_HEAD_COMPANIONS", "Companions"),
                ("LBL_GAMEINFO", "Save: Endar Spire"),
                ("LBL_GAMEINFO", "Area: Command Module"),
                ("LBL_GAMEINFO", "Game: Knights of the Old Republic"),
                ("LBL_GAMEINFO", "Saved: 2026-11-14"),
                ("LBL_GAMEINFO", "Party: Dax, Carth Onasi"),
                ("LBL_SLOT0", "Host"),
                ("LBL_SLOT1", "Choosing, 32 ms"),
                ("LBL_SLOT2", "Ready, 48 ms"),
                ("LBL_CNAME0", "Mia"),
                ("LBL_CNAME2", "Free"),
                ("LBL_CNAME3", "Not available"),
                ("BTN_BACK", "Leave"),
                ("BTN_DONE", if host { "Start Game" } else { "Ready" }),
            ];
            let missing: Vec<String> = (wanted.iter())
                .filter(|(tag, part)| !words(&vm, None, tag).contains(part))
                .map(|(tag, part)| format!("{tag} lacks {part:?}"))
                .collect();
            let host_only = ["BTN_ACCEPT", "BTN_INVITE", "LBL_ADDRESS"];
            let hidden: Vec<bool> = (host_only.iter())
                .map(|tag| state(&mut Lobby::default(), &vm, None, tag).hidden)
                .collect();
            let ok = missing.is_empty() && hidden.iter().all(|hidden| *hidden != host);
            check(
                if host {
                    "lobby_words_host"
                } else {
                    "lobby_words_guest"
                },
                if ok {
                    Ok(format!("{view}: every row shown"))
                } else {
                    Err(format!("{view}: {missing:?}, host-only hidden {hidden:?}"))
                },
            )
        })
        .collect()
}

/// What each press asks for, in each view: a guest's settings rows are
/// greyed and ask nothing, a companion the story took asks nothing, a
/// guest's choice of another guest's companion is a choice, Start Game is
/// the host's only, and a guest's Ready marks Ready.
fn presses() -> Check {
    let host = sample(0, false);
    let guest = sample(2, false);
    let greyed = |vm: &ViewModel, tag| state(&mut Lobby::default(), vm, None, tag).greyed;
    let cases: [(&str, &ViewModel, &str, Vec<Intent>); 10] = [
        (
            "host saber",
            &host,
            "BTN_SABER",
            lobby(LobbyIntent::SaberRules("ja".into())),
        ),
        (
            "host collision",
            &host,
            "BTN_COLLISION",
            lobby(LobbyIntent::Collision("restricted".into())),
        ),
        ("guest saber", &guest, "BTN_SABER", Vec::new()),
        ("guest collision", &guest, "BTN_COLLISION", Vec::new()),
        ("host taken away", &host, "BTN_NPC3", Vec::new()),
        ("guest taken away", &guest, "BTN_NPC3", Vec::new()),
        (
            "guest another's",
            &guest,
            "BTN_NPC0",
            lobby(LobbyIntent::Choose(0)),
        ),
        ("host free", &host, "BTN_NPC2", lobby(LobbyIntent::Fill(2))),
        ("host start", &host, "BTN_DONE", lobby(LobbyIntent::Start)),
        (
            "guest ready",
            &guest,
            "BTN_DONE",
            lobby(LobbyIntent::Ready(false)),
        ),
    ];
    let wrong: Vec<String> = (cases.iter())
        .filter_map(|(name, vm, tag, want)| {
            let got = pressed(vm, tag);
            (got != *want).then(|| format!("{name}: {got:?}"))
        })
        .collect();
    let greys = [
        greyed(&guest, "BTN_SABER"),
        greyed(&guest, "BTN_COLLISION"),
        greyed(&host, "BTN_SABER"),
    ];
    let ok = wrong.is_empty() && greys == [true, true, false];
    check(
        "lobby_presses",
        if ok {
            Ok(format!("{} presses as wanted", cases.len()))
        } else {
            Err(format!("{wrong:?}, greyed {greys:?}"))
        },
    )
}

/// The description strip with the cursor on each piece.
fn descriptions() -> Check {
    let (host, guest, mia) = (sample(0, false), sample(2, false), sample(1, false));
    let cases: [(&str, &ViewModel, &str, &str); 6] = [
        (
            "swap",
            &guest,
            "BTN_NPC0",
            "Carth Onasi is played by Mia. Choose them to swap characters with Mia.",
        ),
        (
            "taken away",
            &guest,
            "BTN_NPC3",
            "Zaalbar is not available in this part of the story.",
        ),
        (
            "free",
            &mia,
            "BTN_NPC2",
            "Mission Vao is free. Choose them to play as Mission Vao.",
        ),
        (
            "host fills",
            &host,
            "BTN_NPC2",
            "Choose Mission Vao to add them to the party.",
        ),
        (
            "guest setting",
            &guest,
            "BTN_SABER",
            "The host chooses the saber rules and the collision.",
        ),
        (
            "host setting",
            &host,
            "BTN_COLLISION",
            "Restricted: Characters collide",
        ),
    ];
    let wrong: Vec<String> = (cases.iter())
        .filter_map(|(name, vm, focus, part)| {
            let said = words(vm, Some(focus), "LBL_DESC");
            (!said.contains(part)).then(|| format!("{name}: {said:?}"))
        })
        .collect();
    check(
        "lobby_descriptions",
        if wrong.is_empty() {
            Ok(format!("{} descriptions as wanted", cases.len()))
        } else {
            Err(format!("{wrong:?}"))
        },
    )
}

/// A save with no companions: the guests' slots say "Remote droid", no
/// companion place shows, and Start Game still starts.
fn droids() -> Check {
    let vm = no_companions();
    let mut host = no_companions();
    if let Some(lobby) = host.lobby.as_mut() {
        lobby.host = true;
        lobby.own_slot = 0;
    }
    let slots = [words(&vm, None, "LBL_SLOT1"), words(&vm, None, "LBL_SLOT2")];
    let row_hidden = state(&mut Lobby::default(), &vm, None, "BTN_NPC0").hidden;
    let start = pressed(&host, "BTN_DONE");
    let ok = slots.iter().all(|slot| slot.contains("Remote droid"))
        && row_hidden
        && pressed(&vm, "BTN_NPC0").is_empty()
        && start == lobby(LobbyIntent::Start);
    check(
        "lobby_without_companions",
        if ok {
            Ok("the guests fly droids".into())
        } else {
            Err(format!(
                "slots {slots:?}, row hidden {row_hidden}, start {start:?}"
            ))
        },
    )
}

/// Movie Duels' saber rules are offered only with it installed: the host's
/// press from Jedi Academy's rules stays on them without it, and goes on to
/// Movie Duels Advanced with it.
fn movie_duels() -> Check {
    let without = pressed(&sample(0, false), "BTN_SABER");
    let with = pressed(&sample(0, true), "BTN_SABER");
    let ok = without == lobby(LobbyIntent::SaberRules("ja".into()))
        && with == lobby(LobbyIntent::SaberRules("mda".into()));
    check(
        "lobby_movie_duels_rules",
        if ok {
            Ok("offered only with Movie Duels".into())
        } else {
            Err(format!("without {without:?}, with {with:?}"))
        },
    )
}

/// The screen built on a layout holding the party selection's tags: its
/// own pieces are made, the companion row is laid out in one line, and the
/// pieces the lobby does not use are hidden.
fn built() -> Check {
    let mut assets = Fake::new();
    let tags: Vec<String> = ["BTN_NPC", "LBL_CHAR", "LBL_NA"]
        .iter()
        .flat_map(|prefix| (0..12).map(move |at| format!("{prefix}{at}")))
        .chain(
            [
                "LBL_NAMEBACK",
                "LBL_NPC_NAME",
                "LBL_TITLE",
                "LBL_3D",
                "BTN_ACCEPT",
                "BTN_BACK",
                "BTN_DONE",
            ]
            .map(String::from),
        )
        .collect();
    let controls = (tags.iter().enumerate())
        .map(|(at, tag)| button(tag, [20, 10 + at as i32 * 4, 100, 10], b""))
        .collect();
    assets
        .layouts
        .insert("partyselection", root(800, 600, controls));
    let vm = sample(0, false);
    let stack = opened("lobby", &mut assets, &vm);
    let made = [
        "LBL_GAMEINFO",
        "BTN_SABER",
        "BTN_COLLISION",
        "LBL_DESC",
        "LBL_SLOT2",
        "LBL_CNAME11",
        "BTN_INVITE",
    ];
    let missing: Vec<&str> = made
        .iter()
        .filter(|tag| stack.extent_of(tag).is_none())
        .copied()
        .collect();
    let tops: Vec<i32> = (0..4)
        .filter_map(|at| {
            stack
                .extent_of(&format!("BTN_NPC{at}"))
                .map(|extent| extent.top)
        })
        .collect();
    let one_row = tops.windows(2).all(|pair| pair[0] == pair[1]) && tops.len() == 4;
    let unused = stack.hidden("LBL_3D", &vm);
    check(
        "lobby_built",
        if missing.is_empty() && one_row && unused {
            Ok("the lobby's pieces are made from the party selection's".into())
        } else {
            Err(format!(
                "missing {missing:?}, row tops {tops:?}, 3D view hidden {unused}"
            ))
        },
    )
}

pub(super) fn all() -> Vec<Check> {
    let mut checks = words_shown();
    checks.extend([presses(), descriptions(), droids(), movie_duels(), built()]);
    checks
}
