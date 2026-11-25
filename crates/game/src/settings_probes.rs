//! The probes of the bindings (`kjk sim probes`, group "bindings") and of
//! the settings' file (group "settings file").

use super::*;
use sim::probe::check;

pub(super) fn checks() -> sim::probe::Checks {
    vec![
        ("input names", input_names()),
        ("chord names", chord_names()),
        ("chords read", chords_read()),
        ("reversed mouse", reversed_mouse()),
        ("bindings refused", refused()),
        ("pad reset", pad_reset()),
        ("feedback defaults", feedback_reset()),
        ("pad rows", pad_rows()),
        ("block shares the right button", block_shares()),
        ("shared button", shared_button()),
        ("bar defaults", bar_defaults()),
        ("unbound menu and interface keys", unbound_defaults()),
        ("photo mode key", photo_mode_key()),
        ("aim defaults", aim_defaults()),
        ("aim command", aim_command()),
        ("layout 8", layout_8()),
        ("old layout file", old_layout_file()),
        ("mouse button binding", mouse_button_binding()),
        ("old layout 4 pad file", old_pad_file()),
        ("old files get slots 7 to 10", added_slots_file()),
        ("lb layer", lb_layer()),
    ]
}

/// The settings `file` holds, loaded as a start loads them, and the file as
/// the load saved it back. The file is written to a folder of its own named
/// `name`, which is removed afterwards.
fn loaded(name: &str, file: &[(&str, &str)]) -> (Settings, BTreeMap<String, String>) {
    let folder = std::env::temp_dir().join(format!("kjk-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let path = folder.join("settings.json");
    let values: BTreeMap<&str, &str> = file.iter().copied().collect();
    let _ = std::fs::write(
        &path,
        serde_json::to_string(&values).expect("strings serialize"),
    );
    let settings = Settings::load_from(Some(path.clone()));
    let saved = std::fs::read(&path)
        .ok()
        .and_then(|text| serde_json::from_slice(&text).ok())
        .unwrap_or_else(BTreeMap::new);
    let _ = std::fs::remove_dir_all(&folder);
    (settings, saved)
}

/// The pairs of rows bound to one key or chord that `SHARES` does not let
/// share it. An unbound row clashes with nothing.
fn clashes(rows: &[KeyRow]) -> Vec<String> {
    rows.iter()
        .enumerate()
        .flat_map(|(at, row)| {
            rows[at + 1..]
                .iter()
                .filter(move |other| {
                    !row.key.is_empty()
                        && other.key == row.key
                        && !row.shares.contains(&other.action)
                })
                .map(move |other| format!("{} and {} on {}", row.action, other.action, row.key))
        })
        .collect()
}

/// The key and the chord of the row for the action named `action`, each
/// empty where none is bound, or none where no row has that name.
fn bound(settings: &Settings, action: &str) -> Option<(String, String)> {
    let find = |rows: Vec<KeyRow>| {
        rows.into_iter()
            .find(|row| row.action == action)
            .map(|row| row.key)
    };
    let key = find(settings.rows())?;
    Some((key, find(settings.pad_rows()).unwrap_or_else(String::new)))
}

/// The rows of `wanted` that `settings` does not show with that label, key
/// and chord. Each row of `wanted` is a settings name, a label, a key and a
/// chord, with an empty key or chord for none.
fn unlike_defaults(settings: &Settings, wanted: &[(&str, &str, &str, &str)]) -> Vec<String> {
    let rows = settings.rows();
    wanted
        .iter()
        .filter(|(name, label, key, chord)| {
            let labelled = rows
                .iter()
                .any(|row| row.action == *name && row.label == *label);
            !labelled || bound(settings, name) != Some((key.to_string(), chord.to_string()))
        })
        .map(|(name, ..)| format!("{name} {:?}", bound(settings, name)))
        .collect()
}

/// A fresh settings file leaves the keys that open the menu's Equipment,
/// Character and Messages pages, and the key that hides the interface,
/// with no key and no chord. Each has a row of its own on the Controls
/// screen, so the player can bind it there, on the keyboard's tab and on
/// the controller's.
fn unbound_defaults() -> Result<String, String> {
    let settings = Settings::defaults();
    let wanted = [
        ("equipment", "Equipment", "", ""),
        ("character", "Character", "", ""),
        ("messages", "Messages", "", ""),
        ("hide_interface", "Hide interface", "", ""),
    ];
    let wrong = unlike_defaults(&settings, &wanted);
    let pads = settings.pad_rows();
    let on_pad: Vec<&str> = wanted
        .iter()
        .map(|(name, ..)| *name)
        .filter(|name| !pads.iter().any(|row| row.action == *name))
        .collect();
    check(
        wrong.is_empty() && on_pad.is_empty(),
        format!("wrong {wrong:?}, no controller row {on_pad:?}"),
    )
}

/// A fresh settings file puts Photo mode on F2, with no chord on the
/// controller, and its row on both of the Controls screen's tabs. Every
/// action has a saved name of its own and a label.
fn photo_mode_key() -> Result<String, String> {
    let settings = Settings::defaults();
    let wrong = unlike_defaults(&settings, &[("photo_mode", "Photo mode", "F2", "")]);
    let names: std::collections::BTreeSet<&str> =
        ACTIONS.iter().map(|(_, name, ..)| *name).collect();
    let unlabelled: Vec<&str> = (ACTIONS.iter())
        .filter(|(_, name, label, ..)| name.is_empty() || label.is_empty())
        .map(|(_, name, ..)| *name)
        .collect();
    check(
        wrong.is_empty() && names.len() == ACTIONS.len() && unlabelled.is_empty(),
        format!(
            "wrong {wrong:?}, {} names for {} actions, unlabelled {unlabelled:?}",
            names.len(),
            ACTIONS.len()
        ),
    )
}

/// The pairs of rows of both devices bound to one key or chord that `SHARES`
/// does not let share it.
fn all_clashes(settings: &Settings) -> Vec<String> {
    let mut clashed = clashes(&settings.rows());
    clashed.extend(clashes(&settings.pad_rows()));
    clashed
}

/// A fresh settings file puts Aim on the right mouse button and the left
/// trigger, where the alternate attack and, on the mouse, Block are too. It
/// puts the saber throw, kata and ranged feat action on R and on LB with
/// the right trigger. No row of Feat attack is left. No key or chord is on
/// two actions unless `SHARES` lets them share it.
fn aim_defaults() -> Result<String, String> {
    let settings = Settings::defaults();
    let mut wrong = unlike_defaults(
        &settings,
        &[
            ("aim", "Aim", "MouseRight", "LeftTrigger2"),
            ("alt_attack", "Alt attack", "MouseRight", "LeftTrigger2"),
            (
                "duel_alt_attack",
                "Saber throw/kata/ranged feat",
                "KeyR",
                "LB+RightTrigger2",
            ),
        ],
    );
    if bound(&settings, "feat_attack").is_some() {
        wrong.push("a row of Feat attack".into());
    }
    let clashed = all_clashes(&settings);
    check(
        wrong.is_empty() && clashed.is_empty(),
        format!("wrong {wrong:?}, clashes {clashed:?}"),
    )
}

/// The command the player's held actions make (`sim_host::player_command`),
/// with a blaster, a lightsaber, a melee blade or nothing in hand. Aim held
/// sends the aim hold. The saber throw, kata and ranged feat action sends
/// the feat hold with a blaster and the alternate attack with anything
/// else. A pad's chord that is also the attack under the block
/// (`Actions::feat_under_block`) sends the feat hold with a blaster and
/// nothing with anything else. With the alternate attack, Block and Aim
/// held on one shared button and a blaster in hand, the command carries the
/// alternate attack and the aim and no block.
fn aim_command() -> Result<String, String> {
    use crate::sim_host::{InHand, Look, player_command};
    use sim::combat::saber::SaberRules::DuelAdvanced;
    #[derive(Debug, Clone, Copy)]
    enum Hand {
        Blaster,
        Lightsaber,
        Blade,
        Empty,
    }
    let command = |hand: Hand, held: &[Action], under_block: bool| {
        let mut actions = crate::pad::Actions::default();
        actions.block_shared = true;
        actions.feat_under_block = under_block;
        held.iter()
            .for_each(|action| actions.set_held(*action, true));
        let look = Look {
            pitch: 0.0,
            yaw: 0.0,
        };
        let uses = (&Default::default(), &Default::default());
        let in_hand = InHand {
            rules: DuelAdvanced,
            blade: matches!(hand, Hand::Lightsaber | Hand::Blade),
            blaster: matches!(hand, Hand::Blaster),
        };
        let sent = player_command(&actions, look, uses, || in_hand);
        let has = |bits: u8, bit: u8| bits & bit != 0;
        [
            sent.buttons & sim::buttons::ALT_ATTACK != 0,
            has(sent.holds, sim::holds::AIM),
            has(sent.holds, sim::holds::FEAT),
            has(sent.holds, sim::holds::BLOCK),
        ]
    };
    let throw: &[Action] = &[Action::DuelAltAttack];
    let blocking: &[Action] = &[Action::Attack, Action::Block];
    // Each case: what is in hand, what is held, whether the pad's chord of
    // the attack under the block is held, and whether the command then
    // carries the alternate attack, the aim, the feat attack and the block.
    let cases: [(Hand, &[Action], bool, [bool; 4]); 11] = [
        (
            Hand::Blaster,
            &[Action::Aim],
            false,
            [false, true, false, false],
        ),
        (Hand::Blaster, throw, false, [false, false, true, false]),
        (Hand::Lightsaber, throw, false, [true, false, false, false]),
        (Hand::Blade, throw, false, [true, false, false, false]),
        (Hand::Empty, throw, false, [true, false, false, false]),
        (Hand::Blaster, blocking, true, [false, false, true, true]),
        (
            Hand::Lightsaber,
            blocking,
            true,
            [false, false, false, true],
        ),
        (Hand::Blade, blocking, true, [false, false, false, true]),
        (Hand::Empty, blocking, true, [false, false, false, true]),
        (
            Hand::Blaster,
            &[Action::AltAttack, Action::Block, Action::Aim],
            false,
            [true, true, false, false],
        ),
        (
            Hand::Lightsaber,
            &[Action::DuelAltAttack, Action::Attack],
            false,
            [true, false, false, false],
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(hand, held, under_block, wanted)| {
            let sent = command(*hand, held, *under_block);
            (sent != *wanted).then(|| {
                format!(
                    "{hand:?} {held:?} under block {under_block}: (alt, aim, feat, block) {sent:?}"
                )
            })
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}

/// The old files that `layout_8` loads, each with its name, its layout, its
/// bindings, and the bindings it then has: the keys of Aim, the alternate
/// attack and the saber throw, kata and ranged feat action, and the chords
/// of Aim and that action. An empty binding is none.
type OldFile = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    [&'static str; 5],
);

/// The bindings an old file with nothing rebound on these rows ends with.
const UNTOUCHED: [&str; 5] = [
    "MouseRight",
    "MouseRight",
    "KeyR",
    "LeftTrigger2",
    "LB+RightTrigger2",
];

const OLD_FILES: [OldFile; 15] = [
    ("nothing rebound", "5", &[], UNTOUCHED),
    (
        "a removed action's key left on R",
        "5",
        &[("key.lightning", "KeyR")],
        UNTOUCHED,
    ),
    (
        "a reversed mouse",
        "5",
        &[
            ("key.attack", "MouseRight"),
            ("key.alt_attack", "MouseLeft"),
        ],
        [
            "MouseLeft",
            "MouseLeft",
            "KeyR",
            "LeftTrigger2",
            "LB+RightTrigger2",
        ],
    ),
    (
        "Kick on R",
        "5",
        &[("key.kick", "KeyR"), ("key.duel_alt_attack", "KeyG")],
        [
            "MouseRight",
            "MouseRight",
            "KeyG",
            "LeftTrigger2",
            "LB+RightTrigger2",
        ],
    ),
    (
        "Use on the right mouse button",
        "5",
        &[
            ("key.use", "MouseRight"),
            ("key.alt_attack", "none"),
            ("key.block", "none"),
        ],
        ["", "", "KeyR", "LeftTrigger2", "LB+RightTrigger2"],
    ),
    (
        "the alternate attack on Y",
        "5",
        &[("pad.alt_attack", "North"), ("pad.leader", "LeftTrigger2")],
        [
            "MouseRight",
            "MouseRight",
            "KeyR",
            "North",
            "LB+RightTrigger2",
        ],
    ),
    ("nothing rebound", "6", &[], UNTOUCHED),
    (
        "Feat attack on O, Saber throw and kata unbound",
        "6",
        &[("key.feat_attack", "KeyO"), ("key.duel_alt_attack", "none")],
        [
            "MouseRight",
            "MouseRight",
            "KeyO",
            "LeftTrigger2",
            "LB+RightTrigger2",
        ],
    ),
    (
        "both rebound",
        "6",
        &[
            ("key.feat_attack", "KeyO"),
            ("key.duel_alt_attack", "F3"),
            ("pad.feat_attack", "LB+DPadRight"),
            ("pad.duel_alt_attack", "LB+DPadLeft"),
        ],
        [
            "MouseRight",
            "MouseRight",
            "F3",
            "LeftTrigger2",
            "LB+DPadLeft",
        ],
    ),
    (
        "Feat attack's chord moved, Saber throw and kata without one",
        "6",
        &[
            ("pad.feat_attack", "LB+DPadLeft"),
            ("pad.duel_alt_attack", "none"),
        ],
        [
            "MouseRight",
            "MouseRight",
            "KeyR",
            "LeftTrigger2",
            "LB+DPadLeft",
        ],
    ),
    (
        "both unbound, Kick on R",
        "6",
        &[
            ("key.feat_attack", "none"),
            ("key.duel_alt_attack", "none"),
            ("key.kick", "KeyR"),
        ],
        [
            "MouseRight",
            "MouseRight",
            "",
            "LeftTrigger2",
            "LB+RightTrigger2",
        ],
    ),
    (
        "Saber throw and kata on R, Feat attack unbound, an old Force key on R",
        "6",
        &[
            ("key.duel_alt_attack", "KeyR"),
            ("pad.duel_alt_attack", "none"),
            ("key.feat_attack", "none"),
            ("key.lightning", "KeyR"),
        ],
        UNTOUCHED,
    ),
    (
        "the action bar's layout 6 with a reversed mouse",
        "6",
        &[
            ("key.attack", "MouseRight"),
            ("key.alt_attack", "MouseLeft"),
            ("key.bar_7", "Digit7"),
        ],
        [
            "MouseLeft",
            "MouseLeft",
            "KeyR",
            "LeftTrigger2",
            "LB+RightTrigger2",
        ],
    ),
    ("nothing rebound", "7", &[], UNTOUCHED),
    (
        "Aim on the left mouse button, the merged action unbound",
        "7",
        &[
            ("key.aim", "MouseLeft"),
            ("key.attack", "MouseRight"),
            ("key.alt_attack", "MouseLeft"),
            ("key.duel_alt_attack", "none"),
            ("pad.duel_alt_attack", "LB+DPadLeft"),
        ],
        ["MouseLeft", "MouseLeft", "", "LeftTrigger2", "LB+DPadLeft"],
    ),
];

/// A file of layout 5, 6 or 7 loads as layout 8 (`OLD_FILES`). The files
/// of layout 6 are the action bar's layout 6 and the test builds' layout 6,
/// which had Aim and Feat attack. The files of layout 7 are the test builds'
/// layout 7, which had Feat attack merged already and keep what they bind.
/// The alternate attack keeps its binding. Aim takes the alternate attack's
/// key or chord where a file of layout 5 or 6 names one and no Aim, and its
/// default otherwise. On each
/// device, the saber throw, kata and ranged feat action keeps the binding of
/// Saber throw and kata where the file names one that is not unbound. Then
/// it takes the binding of Feat attack where the file names one that is not
/// unbound, and its default otherwise. An action is left unbound where
/// another action of the file is on its default. The saved file names no
/// Feat attack. No key or chord ends on two actions unless `SHARES` lets
/// them share it.
fn layout_8() -> Result<String, String> {
    let wrong: Vec<String> = OLD_FILES
        .iter()
        .filter_map(|(name, old, bindings, wanted)| {
            let file: Vec<(&str, &str)> = [("keys.layout", *old)]
                .into_iter()
                .chain(bindings.iter().copied())
                .collect();
            let (settings, saved) = loaded("old-layout", &file);
            let [aim, alt, merged] = ["aim", "alt_attack", "duel_alt_attack"].map(|name| {
                bound(&settings, name).unwrap_or_else(|| (String::new(), String::new()))
            });
            let seen = [
                aim.0.as_str(),
                alt.0.as_str(),
                merged.0.as_str(),
                aim.1.as_str(),
                merged.1.as_str(),
            ];
            let clashed = all_clashes(&settings);
            let layout = saved.get("keys.layout").cloned();
            let feat: Vec<&String> = saved.keys().filter(|key| key.ends_with(".feat_attack")).collect();
            (seen != *wanted
                || !clashed.is_empty()
                || !feat.is_empty()
                || layout.as_deref() != Some(KEY_LAYOUT.1))
            .then(|| {
                format!(
                    "{name} (layout {old}): bound {seen:?}, clashes {clashed:?}, left {feat:?}, saved layout {layout:?}"
                )
            })
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} files, wrong {wrong:?}", OLD_FILES.len()),
    )
}

/// A fresh settings file puts the ten action bar slots on the keys 1 to 9
/// and 0,
/// the power wheel on F, and the hands and the next feat attack on the keys
/// the removed actions left. On the pad, LB with A, B, X and Y uses the
/// first four slots, RB alone opens the wheel, LB alone blocks, and the
/// D-pad's up alone is the next feat attack. The bar setup has no key and
/// no chord. No key or chord is on two actions unless `SHARES` lets them
/// share it.
fn bar_defaults() -> Result<String, String> {
    let settings = Settings::defaults();
    let wanted = [
        ("bar_1", "Action bar slot 1", "Digit1", "LB+South"),
        ("bar_2", "Action bar slot 2", "Digit2", "LB+East"),
        ("bar_3", "Action bar slot 3", "Digit3", "LB+West"),
        ("bar_4", "Action bar slot 4", "Digit4", "LB+North"),
        ("bar_5", "Action bar slot 5", "Digit5", ""),
        ("bar_6", "Action bar slot 6", "Digit6", ""),
        ("bar_7", "Action bar slot 7", "Digit7", ""),
        ("bar_8", "Action bar slot 8", "Digit8", ""),
        ("bar_9", "Action bar slot 9", "Digit9", ""),
        ("bar_10", "Action bar slot 10", "Digit0", ""),
        ("wheel", "Power wheel", "KeyF", "RightTrigger"),
        ("bar_setup", "Action bar setup", "", ""),
        ("right_hand", "Right hand", "BracketRight", ""),
        ("left_hand", "Left hand", "BracketLeft", ""),
        ("next_feat", "Next feat attack", "KeyT", "DPadUp"),
        (
            "block",
            "Block (Advanced rules)",
            "MouseRight",
            "LeftTrigger",
        ),
    ];
    let rows = settings.rows();
    let wrong = unlike_defaults(&settings, &wanted);
    let removed = [
        "power",
        "next_power",
        "previous_power",
        "medpac",
        "stim",
        "shield",
        "grenade",
        "lay_mine",
    ];
    let left: Vec<&str> = removed
        .into_iter()
        .filter(|name| rows.iter().any(|row| row.action == *name))
        .collect();
    let clashed = all_clashes(&settings);
    check(
        wrong.is_empty() && left.is_empty() && clashed.is_empty(),
        format!("wrong {wrong:?}, removed still listed {left:?}, clashes {clashed:?}"),
    )
}

/// A file of layout 3 that bound the medpac to 3, the selected power to F,
/// Use to 1 and the right hand to E loads. Use stays on 1, so the first bar
/// slot has no key; the third slot and the wheel take the keys the medpac
/// and the power left. The left hand and the next feat attack, kept on their
/// old defaults, move to the new ones. Layout 4 dropped the chords with RB
/// as their button, so Use, rebound to RB alone, gets back its default
/// chord, X, and Security, rebound to LB with RB, gets back its default, LB
/// with the D-pad's down. Layout 5 put the next feat attack on the D-pad's
/// up and the third bar slot on LB with X. So Jump, rebound to the D-pad's
/// up, and Draw or holster weapon, rebound to LB with X, get back their
/// defaults, A and the D-pad's down. The wheel takes its default, RB.
/// Nothing ends on two actions, and the file is saved with the layout now
/// (`KEY_LAYOUT`) and without the removed actions.
fn old_layout_file() -> Result<String, String> {
    let (settings, saved) = loaded(
        "old-layout",
        &[
            ("keys.layout", "3"),
            ("key.medpac", "Digit3"),
            ("key.power", "KeyF"),
            ("key.use", "Digit1"),
            ("key.right_hand", "KeyE"),
            ("key.left_hand", "Digit2"),
            ("key.next_feat", "Digit6"),
            ("pad.jump", "DPadUp"),
            ("pad.next_power", "South"),
            ("pad.use", "RightTrigger"),
            ("pad.security", "LB+RightTrigger"),
            ("pad.holster", "LB+West"),
        ],
    );
    let wanted = [
        ("key.use", "Digit1"),
        ("key.bar_1", ""),
        ("key.bar_3", "Digit3"),
        ("key.wheel", "KeyF"),
        ("key.right_hand", "KeyE"),
        ("key.left_hand", "BracketLeft"),
        ("key.next_feat", "KeyT"),
        ("pad.jump", "South"),
        ("pad.wheel", "RightTrigger"),
        ("pad.use", "West"),
        ("pad.security", "LB+DPadDown"),
        ("pad.holster", "DPadDown"),
    ];
    let seen = wanted.map(|(setting, _)| {
        let (device, name) = setting.split_once('.').expect("a binding's setting");
        bound(&settings, name).map(|(key, chord)| if device == "key" { key } else { chord })
    });
    let wanted = wanted.map(|(_, binding)| Some(binding.to_string()));
    let clashed = all_clashes(&settings);
    let layout = saved.get("keys.layout").cloned();
    let retired = ["key.medpac", "key.power", "pad.next_power"]
        .iter()
        .all(|name| !saved.contains_key(*name));
    check(
        seen == wanted && clashed.is_empty() && layout.as_deref() == Some(KEY_LAYOUT.1) && retired,
        format!(
            "bound {seen:?}, clashes {clashed:?}, saved layout {layout:?}, removed ones dropped {retired}"
        ),
    )
}

/// The first bar slot on the mouse's back side button and the wheel on its
/// forward one load from a file and show in the key mapping's rows.
fn mouse_button_binding() -> Result<String, String> {
    let (settings, _) = loaded(
        "mouse-bar",
        &[
            ("keys.layout", "4"),
            ("key.bar_1", "MouseBack"),
            ("key.wheel", "MouseForward"),
        ],
    );
    let seen = ["bar_1", "wheel"].map(|name| bound(&settings, name).map(|(key, _)| key));
    check(
        seen == [Some("MouseBack".into()), Some("MouseForward".into())],
        format!("bound {seen:?}"),
    )
}

/// A file of layout 5, 6 or 7 that bound the map to 7 and names no slots
/// 7 to 10 loads. A file of layout 5 is older than the slots. The test
/// builds wrote files of layouts 6 and 7 without the slots. The map stays
/// on 7, so the seventh bar slot has no key. The eighth, ninth and tenth
/// slots take the keys 8, 9 and 0. No key ends on two actions, and the file
/// is saved with the layout now (`KEY_LAYOUT`).
fn added_slots_file() -> Result<String, String> {
    let wanted = [
        ("map", "Digit7"),
        ("bar_7", ""),
        ("bar_8", "Digit8"),
        ("bar_9", "Digit9"),
        ("bar_10", "Digit0"),
    ];
    let layouts = ["5", "6", "7"];
    let wrong: Vec<String> = layouts
        .iter()
        .filter_map(|old| {
            let (settings, saved) = loaded(
                "added-slots",
                &[("keys.layout", old), ("key.map", "Digit7")],
            );
            let seen = wanted.map(|(name, _)| bound(&settings, name).map(|(key, _)| key));
            let clashed = clashes(&settings.rows());
            let layout = saved.get("keys.layout").cloned();
            (seen != wanted.map(|(_, key)| Some(key.to_string()))
                || !clashed.is_empty()
                || layout.as_deref() != Some(KEY_LAYOUT.1))
            .then(|| {
                format!(
                    "layout {old}: bound {seen:?}, clashes {clashed:?}, saved layout {layout:?}"
                )
            })
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} files, wrong {wrong:?}", layouts.len()),
    )
}

/// The bindings layout 5 leaves to a file of layout 4, each with its saved
/// chord (empty for none saved) and the chord it has after the load (empty
/// for none). The first bar slot's old default (RB with A) names no chord
/// now, and Draw or holster weapon's old default (LB with A) is the first
/// bar slot's now, so both take their new defaults. Stealth, rebound to RB
/// with the left stick, gets back its default. Next Form, rebound to the
/// D-pad's down, which is now Draw or holster weapon's default, gets back
/// its own. Jump, rebound to LB with Y, which is now the fourth bar slot's,
/// would get back A. Switch party leader was rebound to A, so Jump is left
/// unbound instead.
const LAYOUT_4_PADS: [(&str, &str, &str); 7] = [
    ("pad.bar_1", "RB+South", "LB+South"),
    ("pad.holster", "LB+South", "DPadDown"),
    ("pad.stealth", "RB+LeftThumb", "LeftThumb"),
    ("pad.next_form", "DPadDown", "LB+LeftThumb"),
    ("pad.jump", "LB+North", ""),
    ("pad.leader", "South", "South"),
    ("pad.bar_4", "", "LB+North"),
];

/// A file of layout 4 loads with its controller chords brought to layout 5
/// (`LAYOUT_4_PADS`). No chord ends on two actions, and the file is saved
/// with the layout now (`KEY_LAYOUT`).
fn old_pad_file() -> Result<String, String> {
    let file: Vec<(&str, &str)> = [("keys.layout", "4")]
        .into_iter()
        .chain(
            LAYOUT_4_PADS
                .iter()
                .filter(|(_, saved, _)| !saved.is_empty())
                .map(|(setting, saved, _)| (*setting, *saved)),
        )
        .collect();
    let (settings, saved) = loaded("layout-4-pad", &file);
    let wrong: Vec<String> = LAYOUT_4_PADS
        .iter()
        .filter_map(|(setting, _, wanted)| {
            let name = setting.strip_prefix("pad.")?;
            let seen = bound(&settings, name).map(|(_, chord)| chord);
            (seen.as_deref() != Some(*wanted)).then(|| format!("{name} on {seen:?}"))
        })
        .collect();
    let clashed = clashes(&settings.pad_rows());
    let layout = saved.get("keys.layout").cloned();
    check(
        wrong.is_empty() && clashed.is_empty() && layout.as_deref() == Some(KEY_LAYOUT.1),
        format!("wrong {wrong:?}, clashes {clashed:?}, saved layout {layout:?}"),
    )
}

/// Through play's reading of the pad (`sample`): A with LB held uses the
/// first bar slot and does not jump, and A alone jumps. A with RB held
/// jumps too, because RB is no layer. A keeps the layer it went down on
/// until it is let go, so letting go of LB first never jumps. `LB+South`
/// reads back by its name.
fn lb_layer() -> Result<String, String> {
    use bevy::input::gamepad::GamepadButton::{LeftTrigger as Lb, RightTrigger as Rb, South as A};
    let (mut world, pad) = crate::pad::probes::PadWorld::sampling();
    let bar = pad_action("pad.bar_1");
    // Each case: its buttons pressed and let go in turn, whether each of the
    // first bar slot and Jump is pressed on some frame, and whether Jump is
    // held or pressed on some frame.
    type Case = (
        &'static str,
        &'static [(GamepadButton, bool)],
        [bool; 2],
        bool,
    );
    let cases: [Case; 4] = [
        (
            "LB with A",
            &[(Lb, true), (A, true), (A, false), (Lb, false)],
            [true, false],
            false,
        ),
        ("A alone", &[(A, true), (A, false)], [false, true], true),
        (
            "RB with A",
            &[(Rb, true), (A, true), (A, false), (Rb, false)],
            [false, true],
            true,
        ),
        (
            "LB let go before A",
            &[(Lb, true), (A, true), (Lb, false), (A, false)],
            [true, false],
            false,
        ),
    ];
    let watched = [bar, Some(Action::Jump)];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(name, steps, pressed, jumped)| {
            let frames: Vec<crate::pad::Actions> = steps
                .iter()
                .map(|(button, down)| {
                    world.button(pad, *button, *down);
                    world.actions()
                })
                .collect();
            let seen = watched.map(|action| {
                action.is_some_and(|action| frames.iter().any(|frame| frame.pressed(action)))
            });
            let jump = frames
                .iter()
                .any(|frame| frame.held(Action::Jump) || frame.pressed(Action::Jump));
            (seen != *pressed || jump != *jumped)
                .then(|| format!("{name}: pressed {seen:?}, jumped {jump}"))
        })
        .collect();
    let named = crate::pad::Chord::named("LB+South").map(crate::pad::Chord::name);
    check(
        wrong.is_empty() && named.as_deref() == Some("LB+South"),
        format!(
            "{} cases, wrong (bar slot 1, jump) {wrong:?}; LB+South read as {named:?}",
            cases.len()
        ),
    )
}

pub(super) fn file_checks() -> sim::probe::Checks {
    let folder = std::env::temp_dir().join(format!("kjk-settings-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let checks: sim::probe::Checks = [
        ("settings cut short", &b"{\"display.vsync\": \"tr"[..]),
        (
            "settings of the wrong type",
            &b"{\"display.vsync\": false}"[..],
        ),
        ("settings not text", &[0xff, 0xfe, 0x00][..]),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, (name, bytes))| (name, unreadable(&folder.join(n.to_string()), bytes)))
    .collect();
    let _ = std::fs::remove_dir_all(&folder);
    let mut checks = checks;
    checks.extend([
        ("advanced_rules_need_no_mod", advanced_without_mod()),
        ("old_mda_setting_retired", old_mda_retired()),
        ("stance_sets_setting", stance_sets_setting()),
        ("targeted_blocks_setting_ignored", targeted_blocks_ignored()),
    ]);
    checks
}

/// A saved `advanced` is played with no Movie Duels to look for.
fn advanced_without_mod() -> Result<String, String> {
    let mut settings = Settings::defaults();
    settings
        .values
        .insert(SABER_RULES.into(), "advanced".into());
    let rules = settings.saber_rules();
    check(
        rules == SaberRules::DuelAdvanced,
        format!("game.saber_rules = advanced played as {}", rules.name()),
    )
}

/// A saved `mda` or `md` is rewritten at load: to `advanced` with Movie
/// Duels installed, to `ja` without it. A second load changes nothing.
fn old_mda_retired() -> Result<String, String> {
    let wrong: Vec<String> = [(true, "advanced"), (false, "ja")]
        .into_iter()
        .flat_map(|(movie_duels, wanted)| {
            ["mda", "md"].into_iter().filter_map(move |old| {
                let mut read = BTreeMap::from([(SABER_RULES.to_string(), old.to_string())]);
                let first = retire_mda(&mut read, movie_duels);
                let again = retire_mda(&mut read, movie_duels);
                let now = read.get(SABER_RULES).map(String::as_str);
                (!(first && !again && now == Some(wanted))).then(|| {
                    format!("{old} with Movie Duels {movie_duels}: {first}, {again}, {now:?}")
                })
            })
        })
        .collect();
    check(wrong.is_empty(), wrong.join("; "))
}

/// The stance sets setting turns the sets off, and Movie Duels is still
/// needed to turn them on.
fn stance_sets_setting() -> Result<String, String> {
    let cases = [
        (None, true, true),
        (Some("false"), true, false),
        (Some("true"), true, true),
        (Some("true"), false, false),
    ];
    let wrong: Vec<String> = cases
        .into_iter()
        .filter_map(|(setting, movie_duels, wanted)| {
            let got = crate::stance_sets::wanted_with(None, setting, movie_duels);
            (got != wanted).then(|| format!("{setting:?} with Movie Duels {movie_duels}: {got}"))
        })
        .collect();
    check(wrong.is_empty(), wrong.join("; "))
}

/// Targeted blocks follow only `KJK_TARGETED_BLOCKS`. Only the value 1
/// turns them on. The settings have no default for the key of the option
/// that was removed, so a value a settings file still stores there is never
/// read.
fn targeted_blocks_ignored() -> Result<String, String> {
    let env = [None, Some("1"), Some("0")];
    let got = env.map(|env| (env, targeted_blocks_by(env.map(std::ffi::OsStr::new))));
    let want = env.map(|env| (env, env == Some("1")));
    let unread = !Settings::defaults()
        .values
        .contains_key("game.targeted_blocks");
    check(
        got == want && unread,
        format!(
            "the variable's values and whether they turn targeted blocks on: {got:?}; the option's key has no default: {unread}"
        ),
    )
}

/// A settings' file holding `bytes`, which do not read: the defaults
/// stand, the file is kept as `settings.json.bad` byte for byte, and the
/// defaults saved in its place read back, with no temporary file left.
fn unreadable(folder: &std::path::Path, bytes: &[u8]) -> Result<String, String> {
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let path = folder.join("settings.json");
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let loaded = Settings::load_from(Some(path.clone()));
    let kept = std::fs::read(folder.join("settings.json.bad")).ok();
    let again = std::fs::read(&path)
        .ok()
        .and_then(|text| serde_json::from_slice::<BTreeMap<String, String>>(&text).ok());
    let mut left: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    check(
        loaded.display() == Settings::defaults().display()
            && kept.as_deref() == Some(bytes)
            && again.is_some_and(|again| again.contains_key(HOST_ID))
            && left == ["settings.json", "settings.json.bad"],
        format!("kept {}, left {left:?}", kept.is_some()),
    )
}

/// Every key a binding may use and the mouse's buttons, side and further
/// ones among them, read back by their names.
fn input_names() -> Result<String, String> {
    let mice = [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
        MouseButton::Other(8),
    ];
    let inputs: Vec<Input> = KEYS
        .iter()
        .map(|key| Input::Key(*key))
        .chain(mice.map(Input::Mouse))
        .collect();
    let wrong: Vec<String> = inputs
        .iter()
        .filter(|input| Input::named(&input.name()) != Some(**input))
        .map(|input| input.name())
        .collect();
    let refused = Input::named("Escape").is_none() && Input::named("MouseNope").is_none();
    check(
        wrong.is_empty() && refused,
        format!(
            "{} inputs, misread {wrong:?}, Escape refused: {refused}",
            inputs.len()
        ),
    )
}

/// Bindings with modifiers read back by their names, in any order of the
/// modifiers. A modifier named twice, one not known, and a modifier key
/// with modifiers are refused.
fn chord_names() -> Result<String, String> {
    let read = ["Shift+MouseRight", "Ctrl+Alt+KeyE", "Alt+Ctrl+KeyE", "KeyW"]
        .map(|name| Binding::named(name).map(Binding::name));
    let refused = ["Shift+Shift+KeyE", "Hyper+KeyE", "Ctrl+ShiftLeft", "Shift+"]
        .iter()
        .all(|name| Binding::named(name).is_none());
    let shown = ui::screens::key_mapping::key_name("Shift+MouseRight");
    check(
        read == [
            Some("Shift+MouseRight".into()),
            Some("Ctrl+Alt+KeyE".into()),
            Some("Ctrl+Alt+KeyE".into()),
            Some("KeyW".into()),
        ] && refused
            && shown == "Shift + Right mouse",
        format!("read {read:?}, refused {refused}, shown {shown:?}"),
    )
}

/// With Use on Shift + Right mouse, Right mouse alone is the other attack.
/// Shift held with Right mouse is Use and not the other attack, and Left
/// Shift still walks. Leave on Ctrl + L does not leave on L alone.
fn chords_read() -> Result<String, String> {
    let mut map = KeyMap::default();
    let chord = |name| Binding::named(name).expect("a chord");
    map.0.insert(Action::Use, chord("Shift+MouseRight"));
    map.0.insert(Action::LeaveTalk, chord("Ctrl+KeyL"));
    let on = |keys: &[KeyCode]| {
        let (mut board, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        keys.iter().for_each(|key| board.press(*key));
        mouse.press(MouseButton::Right);
        let held: Vec<Action> = map
            .read(Some(&board), Some(&mouse))
            .into_iter()
            .filter(|(action, held, _)| {
                *held && [Action::Use, Action::AltAttack, Action::Walk].contains(action)
            })
            .map(|(action, _, _)| action)
            .collect();
        (held, map.key_pressed(Action::LeaveTalk, &board))
    };
    let alone = on(&[KeyCode::KeyL]);
    let shifted = on(&[KeyCode::ShiftLeft, KeyCode::ControlRight, KeyCode::KeyL]);
    check(
        alone == (vec![Action::AltAttack], false)
            && shifted.0.contains(&Action::Use)
            && shifted.0.contains(&Action::Walk)
            && !shifted.0.contains(&Action::AltAttack)
            && shifted.1,
        format!("alone {alone:?}, with Shift and Ctrl {shifted:?}"),
    )
}

/// A file from before the attacks were bindings, its mouse reversed: the
/// right button attacks, the left is the other mode and blocks, the old
/// switch gone.
fn reversed_mouse() -> Result<String, String> {
    let mut read: BTreeMap<String, String> = [("keys.layout", "2"), ("mouse.reverse", "true")]
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .into_iter()
        .collect();
    let changed = relayout(&mut read) & block_on_alt(&mut read);
    let mut settings = Settings::defaults();
    settings.values.extend(read.clone());
    let keys = settings.keys();
    let [attack, alt, block] = [Action::Attack, Action::AltAttack, Action::Block]
        .map(|action| keys.input(action).map(Input::name));
    check(
        changed
            && attack == Some(Input::Mouse(MouseButton::Right).name())
            && alt == Some(Input::Mouse(MouseButton::Left).name())
            && block == alt
            && !read.contains_key("mouse.reverse"),
        format!("attack {attack:?}, alt {alt:?}, block {block:?}, {read:?}"),
    )
}

/// A key that is none, a chord that is none, a chord with RB as its layer
/// (RB is no layer now), and LB with itself are not taken. `none` unbinds a
/// key and a chord.
fn refused() -> Result<String, String> {
    let mut settings = Settings::defaults();
    settings.apply("key.jump", "Escape");
    settings.apply("key.stealth", "none");
    settings.apply("pad.jump", "Nonsense");
    settings.apply("pad.jump", "RB+South");
    settings.apply("pad.crouch", "LB+LeftTrigger");
    settings.apply("pad.use", "none");
    let (keys, pads) = (settings.keys(), settings.pads());
    check(
        keys.input(Action::Jump) == Some(Input::Key(KeyCode::Space))
            && keys.input(Action::Stealth).is_none()
            && pads
                .chord(Action::Jump)
                .map(crate::pad::Chord::name)
                .as_deref()
                == Some("South")
            && pads
                .chord(Action::Crouch)
                .map(crate::pad::Chord::name)
                .as_deref()
                == Some("East")
            && pads.chord(Action::Use).is_none(),
        format!(
            "jump {:?} / {:?}, stealth {:?}, crouch {:?}, use {:?}",
            keys.input(Action::Jump).map(Input::name),
            pads.chord(Action::Jump).map(crate::pad::Chord::name),
            keys.input(Action::Stealth).map(Input::name),
            pads.chord(Action::Crouch).map(crate::pad::Chord::name),
            pads.chord(Action::Use).map(crate::pad::Chord::name),
        ),
    )
}

/// The controller tab's Default puts the chords back and keeps the pad's
/// feel.
fn pad_reset() -> Result<String, String> {
    let mut settings = Settings::defaults();
    settings.apply("pad.jump", "LB+South");
    settings.apply("pad.sensitivity", "0.90");
    settings.apply("reset", "pad");
    let jump = settings
        .pads()
        .chord(Action::Jump)
        .map(crate::pad::Chord::name);
    let feel = settings.value("pad.sensitivity").map(str::to_string);
    check(
        jump.as_deref() == Some("South") && feel.as_deref() == Some("0.90"),
        format!("jump {jump:?}, sensitivity {feel:?}"),
    )
}

/// The Feedback page's options start as KOTOR 2's, and its Default puts
/// them back. As KOTOR 2's Default does, it also sets the text size back to
/// Auto, and it leaves the other pages' settings alone.
fn feedback_reset() -> Result<String, String> {
    use ui::feedback::{CHOICES, on};
    let fresh = Settings::defaults();
    let started: Vec<bool> = CHOICES.iter().map(|c| on(&fresh.values, c.key)).collect();
    let mut settings = Settings::defaults();
    for choice in CHOICES.iter().filter(|choice| choice.live) {
        settings.apply(choice.key, &(!choice.default).to_string());
    }
    settings.apply(ui::draw::SmallFonts::SETTING, "large");
    settings.apply("camera.invert", "true");
    settings.apply("reset", ui::feedback::PAGE);
    let reset: Vec<bool> = CHOICES
        .iter()
        .map(|c| on(&settings.values, c.key))
        .collect();
    let defaults: Vec<bool> = CHOICES.iter().map(|choice| choice.default).collect();
    let fonts = settings
        .value(ui::draw::SmallFonts::SETTING)
        .map(str::to_string);
    let kept = settings.on("camera.invert");
    check(
        started == defaults && reset == defaults && fonts.as_deref() == Some("auto") && kept,
        format!("started {started:?}, reset {reset:?}, text size {fonts:?}, camera kept {kept}"),
    )
}

/// The controller tab shows every action but the stick's.
fn pad_rows() -> Result<String, String> {
    let rows = Settings::defaults().pad_rows();
    let stick: Vec<&str> = rows
        .iter()
        .map(|row| row.action.as_str())
        .filter(|action| ["forward", "backward", "left", "right", "walk"].contains(action))
        .collect();
    check(
        stick.is_empty() && rows.len() == ACTIONS.len() - STICK.len(),
        format!("{} rows, the stick's among them: {stick:?}", rows.len()),
    )
}

/// Block and the alternate attack are both on the right mouse button by
/// default, and the key mapping lets them keep sharing it: neither row
/// names the other as a clash.
fn block_shares() -> Result<String, String> {
    let rows = Settings::defaults().rows();
    let row = |action: &str| rows.iter().find(|row| row.action == action).cloned();
    let (block, alt) = (row("block"), row("alt_attack"));
    let shared = block
        .as_ref()
        .zip(alt.as_ref())
        .is_some_and(|(block, alt)| {
            block.key == "MouseRight"
                && alt.key == "MouseRight"
                && block.shares.contains(&alt.action)
                && alt.shares.contains(&block.action)
        });
    // The alternate attack's own key is R. A file of the pre-release
    // v0.17.1-md-rc.1 holds Block on R, and its Block moves to the shared
    // button. A file that names both keys keeps them.
    let own = row("duel_alt_attack").map(|row| row.key);
    let block_of = |file: &[(&str, &str)]| {
        let mut read: BTreeMap<String, String> = file
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        block_on_alt(&mut read);
        read.remove("key.block")
    };
    let moved = block_of(&[("key.block", "KeyR"), ("key.alt_attack", "MouseRight")]);
    let kept = block_of(&[
        ("key.block", "KeyR"),
        ("key.alt_attack", "MouseRight"),
        ("key.duel_alt_attack", "KeyV"),
    ]);
    check(
        shared
            && Settings::defaults().keys().input(Action::Block)
                == Some(Input::Mouse(MouseButton::Right))
            && own.as_deref() == Some("KeyR")
            && moved.as_deref() == Some("MouseRight")
            && kept.as_deref() == Some("KeyR"),
        format!(
            "block {:?}, alt attack {:?}, its own key {own:?}, an old file's block {moved:?}, a rebound file's {kept:?}",
            block.map(|row| (row.key, row.shares)),
            alt.map(|row| (row.key, row.shares))
        ),
    )
}

/// Who gets a press of the shared button (`sim_host::share_button`), as a
/// table: the rules, what the body the seat moves holds, whether Block shares the
/// button, and which of the alternate attack and the block the command
/// sends.
fn shared_button() -> Result<String, String> {
    use crate::sim_host::{InHand, share_button};
    use sim::combat::saber::SaberRules::{DuelAdvanced as Mda, Ja};
    let hand = |rules, blade: bool| InHand {
        rules,
        blade,
        blaster: !blade,
    };
    // A lightsaber, lit or put away, and a vibroblade are each a blade
    // (`InHand::of`), whether the player moves their own character or the
    // companion they lead.
    let rows = [
        ("MDA, lightsaber", hand(Mda, true), true, (false, true)),
        ("MDA, blaster", hand(Mda, false), true, (true, false)),
        ("MDA, vibroblade", hand(Mda, true), true, (false, true)),
        ("JA, lightsaber", hand(Ja, true), true, (true, false)),
        ("MDA, Block rebound", hand(Mda, true), false, (true, true)),
        ("JA, Block rebound", hand(Ja, true), false, (true, true)),
    ];
    let pressed = sim::Command {
        buttons: sim::buttons::ALT_ATTACK,
        holds: sim::holds::BLOCK,
        ..sim::Command::default()
    };
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|&(name, held, shared, wanted)| {
            let sent = share_button(pressed, shared, || held);
            let got = (
                sent.buttons & sim::buttons::ALT_ATTACK != 0,
                sent.holds & sim::holds::BLOCK != 0,
            );
            (got != wanted).then(|| format!("{name}: sent (alt, block) {got:?}, wanted {wanted:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} rows, wrong {wrong:?}", rows.len()),
    )
}
