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
    ]
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
    let [attack, alt, block] =
        [Action::Attack, Action::AltAttack, Action::Block].map(|action| keys.input(action));
    check(
        changed
            && attack == Input::Mouse(MouseButton::Right)
            && alt == Input::Mouse(MouseButton::Left)
            && block == alt
            && !read.contains_key("mouse.reverse"),
        format!(
            "attack {}, alt {}, block {}, {read:?}",
            attack.name(),
            alt.name(),
            block.name()
        ),
    )
}

/// A key that is none, a chord that is none, and the shift alone are not
/// taken; `none` unbinds a chord.
fn refused() -> Result<String, String> {
    let mut settings = Settings::defaults();
    settings.apply("key.jump", "Escape");
    settings.apply("pad.jump", "Nonsense");
    settings.apply("pad.crouch", "LeftTrigger");
    settings.apply("pad.use", "none");
    let (keys, pads) = (settings.keys(), settings.pads());
    check(
        keys.input(Action::Jump) == Input::Key(KeyCode::Space)
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
            "jump {} / {:?}, crouch {:?}, use {:?}",
            keys.input(Action::Jump).name(),
            pads.chord(Action::Jump).map(crate::pad::Chord::name),
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
            && Settings::defaults().keys().input(Action::Block) == Input::Mouse(MouseButton::Right)
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
    let hand = |rules, blade| InHand { rules, blade };
    let rows = [
        (
            "MDA, lightsaber, lit or put away, of the seat's character or of a companion it drives",
            hand(Mda, true),
            true,
            (false, true),
        ),
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
