//! The co-op lobby screen (`screens::lobby`) over a sample lobby, in the
//! host's view and in a guest's: the words it shows, what each press asks
//! for, the description strip, and the screen built on a layout holding
//! the party selection's tags.

use super::menus::root;
use super::paint::{Fake, button};
use super::vm_screens::opened;
use super::{Check, check};
use crate::intent::{Intent, LobbyIntent, LobbyPick};
use crate::paint::{ControlState, Painter, Palette};
use crate::screen::Screen;
use crate::screens::lobby::Lobby;
use crate::vm::{
    LobbyCompanionState, LobbyCompanionView, LobbyHeroView, LobbySaveView, LobbySlotState,
    LobbySlotView, LobbyView, ViewModel,
};

/// The host plays the main character, Dax. Carth is played by Mia (slot 1),
/// Bastila by Kalle (slot 2), Mission is free, and Zaalbar is taken away by
/// the story. Seen by the player in `own_slot`. The row's first place is Dax, so Carth is its second.
fn sample(own_slot: usize) -> ViewModel {
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
            hero: Some(LobbyHeroView {
                name: b"Dax".to_vec(),
                portrait: Some("po_pmhc01".into()),
                slot: 0,
                player: "host".into(),
            }),
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
        }),
        ..ViewModel::default()
    }
}

/// The sample after Mia swapped for the main character: the host plays
/// Carth and Mia plays Dax, seen by the player in `own_slot`.
fn mia_plays_dax(own_slot: usize) -> ViewModel {
    let mut vm = sample(own_slot);
    if let Some(lobby) = vm.lobby.as_mut() {
        if let Some(hero) = lobby.hero.as_mut() {
            hero.slot = 1;
            hero.player = "Mia".into();
        }
        lobby.companions[0].state = LobbyCompanionState::Played {
            slot: 0,
            player: "host".into(),
        };
        let [host, mia, _] = &mut lobby.slots;
        if let (Some(host), Some(mia)) = (host, mia) {
            std::mem::swap(&mut host.character, &mut mia.character);
        }
    }
    vm
}

/// The sample with no companions at all (a new game): both guests fly
/// droids.
fn no_companions() -> ViewModel {
    let mut vm = sample(1);
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
            let vm = sample(own);
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
                ("LBL_CNAME0", "Dax"),
                ("LBL_CNAME0", "host"),
                ("LBL_CNAME1", "Mia"),
                ("LBL_CNAME3", "Free"),
                ("LBL_CNAME4", "Zaalbar"),
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
            // A companion the story took away shows its name alone: the
            // empty portrait says the rest.
            let away = words(&vm, None, "LBL_CNAME4");
            let ok = missing.is_empty()
                && hidden.iter().all(|hidden| *hidden != host)
                && away == "Zaalbar";
            check(
                if host {
                    "lobby_words_host"
                } else {
                    "lobby_words_guest"
                },
                if ok {
                    Ok(format!("{view}: every row shown"))
                } else {
                    Err(format!(
                        "{view}: {missing:?}, host-only hidden {hidden:?}, the taken companion's label {away:?}"
                    ))
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
    let host = sample(0);
    let guest = sample(2);
    let greyed = |vm: &ViewModel, tag| state(&mut Lobby::default(), vm, None, tag).greyed;
    let cases: [(&str, &ViewModel, &str, Vec<Intent>); 13] = [
        (
            "host saber",
            &host,
            "BTN_SABER",
            lobby(LobbyIntent::SaberRules("advanced".into())),
        ),
        (
            "host collision",
            &host,
            "BTN_COLLISION",
            lobby(LobbyIntent::Collision("restricted".into())),
        ),
        ("guest saber", &guest, "BTN_SABER", Vec::new()),
        ("guest collision", &guest, "BTN_COLLISION", Vec::new()),
        ("host taken away", &host, "BTN_NPC4", Vec::new()),
        ("guest taken away", &guest, "BTN_NPC4", Vec::new()),
        (
            "guest another's",
            &guest,
            "BTN_NPC1",
            lobby(LobbyIntent::Choose(LobbyPick::Companion(0))),
        ),
        (
            "host a guest's",
            &host,
            "BTN_NPC1",
            lobby(LobbyIntent::Choose(LobbyPick::Companion(0))),
        ),
        (
            "guest main character",
            &guest,
            "BTN_NPC0",
            lobby(LobbyIntent::Choose(LobbyPick::Hero)),
        ),
        ("host own main character", &host, "BTN_NPC0", Vec::new()),
        ("host free", &host, "BTN_NPC3", lobby(LobbyIntent::Fill(2))),
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
    // A companion the story took away shows the layout's "no companion"
    // art in place of the portrait, as KOTOR 2's party selection does.
    let away = [
        state(&mut Lobby::default(), &guest, None, "LBL_CHAR4").hidden,
        !state(&mut Lobby::default(), &guest, None, "LBL_NA4").hidden,
        !state(&mut Lobby::default(), &guest, None, "LBL_CHAR3").hidden,
        state(&mut Lobby::default(), &guest, None, "LBL_NA3").hidden,
    ];
    let ok = wrong.is_empty() && greys == [true, true, false] && away == [true; 4];
    check(
        "lobby_presses",
        if ok {
            Ok(format!("{} presses as wanted", cases.len()))
        } else {
            Err(format!("{wrong:?}, greyed {greys:?}, taken away {away:?}"))
        },
    )
}

/// The description strip with the cursor on each piece.
fn descriptions() -> Check {
    let (host, guest, mia) = (sample(0), sample(2), sample(1));
    let (host_of_mia, mia_hero) = (mia_plays_dax(0), mia_plays_dax(1));
    let cases: [(&str, &ViewModel, &str, &str); 10] = [
        (
            "swap",
            &guest,
            "BTN_NPC1",
            "Carth Onasi is played by Mia. Choose them to swap characters with Mia.",
        ),
        (
            "taken away",
            &guest,
            "BTN_NPC4",
            "Zaalbar is not available in this part of the story.",
        ),
        (
            "free",
            &mia,
            "BTN_NPC3",
            "Mission Vao is free. Choose them to play as Mission Vao.",
        ),
        (
            "host fills",
            &host,
            "BTN_NPC3",
            "Choose Mission Vao to add them to the party.",
        ),
        (
            "main character to a guest",
            &guest,
            "BTN_NPC0",
            "Dax is the main character, played by host. Choose them to swap characters with host. The main character's player answers the story's dialogue.",
        ),
        (
            "main character to its player",
            &host,
            "BTN_NPC0",
            "You play Dax, the main character. You answer the story's dialogue.",
        ),
        (
            "main character to the guest who plays it",
            &mia_hero,
            "BTN_NPC0",
            "You play Dax, the main character. You answer the story's dialogue.",
        ),
        (
            "main character to the host when a guest plays it",
            &host_of_mia,
            "BTN_NPC0",
            "Dax is the main character, played by Mia. Choose them to swap characters with Mia. The main character's player answers the story's dialogue.",
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

/// Mia plays the main character: in the host's view and in hers, the row's
/// first place names her, the host's slot still says "Host" and shows the
/// host's companion, her slot shows the main character, and the host's
/// press on the main character asks to take it back.
fn guest_plays_main_character() -> Vec<Check> {
    VIEWS
        .iter()
        .map(|&(view, _)| {
            let own = if view == "host" { 0 } else { 1 };
            let vm = mia_plays_dax(own);
            let wanted = [
                ("LBL_CNAME0", "Mia"),
                ("LBL_CNAME1", "host"),
                ("LBL_SLOT0", "Host"),
                ("LBL_SLOT0", "Carth Onasi"),
                ("LBL_SLOT1", "Dax"),
                ("LBL_SLOT1", "Soldier, level 4"),
            ];
            let missing: Vec<String> = (wanted.iter())
                .filter(|(tag, part)| !words(&vm, None, tag).contains(part))
                .map(|(tag, part)| format!("{tag} lacks {part:?}"))
                .collect();
            let portrait = state(&mut Lobby::default(), &vm, None, "LBL_SLOTP1").fill;
            let press = pressed(&vm, "BTN_NPC0");
            let want_press = match own {
                0 => lobby(LobbyIntent::Choose(LobbyPick::Hero)),
                _ => Vec::new(),
            };
            let ok = missing.is_empty()
                && portrait.as_deref() == Some("po_pmhc01")
                && press == want_press;
            check(
                if own == 0 {
                    "lobby_guest_plays_main_character_host"
                } else {
                    "lobby_guest_plays_main_character_guest"
                },
                if ok {
                    Ok(format!("{view}: Mia shown with the main character"))
                } else {
                    Err(format!(
                        "{view}: {missing:?}, portrait {portrait:?}, press {press:?}"
                    ))
                },
            )
        })
        .collect()
}

/// A label under the Companions row painted in a column six test-font
/// letters wide (a letter's advance is 1): a word wider than the column is
/// cut with a full stop instead of broken, so no line runs past the column
/// into the next. Mission is renamed "Canderous Ordo" and Carth is free.
fn labels_in_columns() -> Check {
    let mut vm = sample(1);
    if let Some(lobby) = vm.lobby.as_mut() {
        lobby.companions[2].name = b"Canderous Ordo".to_vec();
    }
    let control = fmt_odyssey::gui::Control {
        kind: fmt_odyssey::gui::Kind::Label,
        ..button("LBL_CNAME3", [0, 0, 6, 60], b"")
    };
    let state = Lobby::default().state(&control, &vm);
    let mut fake = Fake::new();
    let shown = Painter::new(&mut fake, Palette::K1).shown_words(&control, &state);
    let shown = String::from_utf8_lossy(&shown).into_owned();
    let words: Vec<&str> = shown.split(['\n', ' ']).collect();
    let ok = words.iter().all(|word| word.len() <= 6) && shown.starts_with("Cande.");
    check(
        "lobby_labels_stay_in_their_columns",
        if ok {
            Ok(format!("{shown:?}"))
        } else {
            Err(format!("{shown:?} has a word wider than its column"))
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
    let row_hidden = state(&mut Lobby::default(), &vm, None, "BTN_NPC1").hidden;
    let start = pressed(&host, "BTN_DONE");
    let ok = slots.iter().all(|slot| slot.contains("Remote droid"))
        && row_hidden
        && pressed(&vm, "BTN_NPC1").is_empty()
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

/// Both combat rules are offered whether or not Movie Duels is installed.
/// The host's press moves the lobby from Jedi Academy's rules to the
/// Advanced rules, and a second press moves it back.
fn combat_rules() -> Check {
    let from_ja = pressed(&sample(0), "BTN_SABER");
    let mut advanced = sample(0);
    if let Some(lobby) = advanced.lobby.as_mut() {
        lobby.saber_rules = "advanced".into();
    }
    let from_advanced = pressed(&advanced, "BTN_SABER");
    let ok = from_ja == lobby(LobbyIntent::SaberRules("advanced".into()))
        && from_advanced == lobby(LobbyIntent::SaberRules("ja".into()));
    check(
        "lobby_combat_rules",
        if ok {
            Ok("Jedi Academy's rules and the Advanced rules are offered in turn".into())
        } else {
            Err(format!(
                "from ja {from_ja:?}, from advanced {from_advanced:?}"
            ))
        },
    )
}

/// Fake assets with a layout holding the party selection's tags.
fn party_layout() -> Fake {
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
    assets
}

/// The left edges of the row's first `places` places, and where the last
/// one ends, in `stack`.
fn row_edges(stack: &crate::screen::Stack, places: usize) -> (Vec<i32>, Option<i32>) {
    let extents: Vec<_> = (0..places)
        .filter_map(|at| stack.extent_of(&format!("BTN_NPC{at}")))
        .collect();
    let lefts = extents.iter().map(|extent| extent.left).collect();
    let end = extents.last().map(|extent| extent.left + extent.width);
    (lefts, end)
}

/// The screen built on a layout holding the party selection's tags: its
/// own pieces are made, the companion row is laid out in one line, and the
/// pieces the lobby does not use are hidden.
fn built() -> Check {
    let mut assets = party_layout();
    let vm = sample(0);
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
    // The eleventh place (the main character and ten companions) ends
    // inside the description strip's panel, which ends at 722 units.
    // A row of fewer than twelve places keeps its places 58 units apart.
    let (lefts, last) = row_edges(&stack, 11);
    let fits = last.is_some_and(|right| right <= 722)
        && lefts.windows(2).all(|pair| pair[1] - pair[0] == 58);
    check(
        "lobby_built",
        if missing.is_empty() && one_row && unused && fits {
            Ok("the lobby's pieces are made from the party selection's".into())
        } else {
            Err(format!(
                "missing {missing:?}, row tops {tops:?}, 3D view hidden {unused}, places at {lefts:?}, the eleventh ends at {last:?}"
            ))
        },
    )
}

/// A lobby of a save whose mod recruited all twelve of KOTOR 2's
/// companions. The row shows the main character and the twelve companions
/// in thirteen places. Its spacing narrows so that the last place still
/// ends inside the description strip's panel, at 722 units, and no two
/// portraits overlap.
fn twelve_companions() -> Check {
    let mut assets = party_layout();
    let mut vm = sample(0);
    if let Some(lobby) = vm.lobby.as_mut() {
        lobby.companions = (0..12)
            .map(|npc| LobbyCompanionView {
                npc,
                name: format!("Companion {npc}").into_bytes(),
                portrait: Some(format!("po_p{npc}")),
                state: LobbyCompanionState::Free,
            })
            .collect();
    }
    let mut stack = opened("lobby", &mut assets, &vm);
    stack.step(&mut assets, &vm, &super::menus::keys(&[]));
    let (lefts, last) = row_edges(&stack, 13);
    let shown = (0..13)
        .filter(|at| !stack.hidden(&format!("LBL_CHAR{at}"), &vm))
        .count();
    let apart = lefts.windows(2).all(|pair| pair[1] - pair[0] >= 38);
    let ok = lefts.len() == 13 && shown == 13 && apart && last.is_some_and(|right| right <= 722);
    check(
        "lobby_twelve_companions",
        if ok {
            Ok(format!("thirteen places at {lefts:?}"))
        } else {
            Err(format!(
                "{shown} portraits shown, places at {lefts:?}, the last ends at {last:?}"
            ))
        },
    )
}

pub(super) fn all() -> Vec<Check> {
    let mut checks = words_shown();
    checks.extend(guest_plays_main_character());
    checks.extend([
        presses(),
        descriptions(),
        droids(),
        combat_rules(),
        built(),
        twelve_companions(),
        labels_in_columns(),
    ]);
    checks
}
