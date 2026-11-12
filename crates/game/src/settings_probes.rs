//! The probes of the bindings (`kjk sim probes`, group "bindings") and of
//! the settings' file (group "settings file").

use super::*;
use sim::probe::check;

pub(super) fn checks() -> sim::probe::Checks {
    vec![
        ("input names", input_names()),
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
    let checks = [
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
    checks
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
    check(
        shared
            && Settings::defaults().keys().input(Action::Block) == Input::Mouse(MouseButton::Right),
        format!(
            "block {:?}, alt attack {:?}",
            block.map(|row| (row.key, row.shares)),
            alt.map(|row| (row.key, row.shares))
        ),
    )
}

/// Who gets a press of the shared button (`sim_host::share_button`), as a
/// table: the rules, what the character holds, whether Block shares the
/// button, and which of the alternate attack and the block the command
/// sends.
fn shared_button() -> Result<String, String> {
    use crate::sim_host::{InHand, share_button};
    use sim::combat::saber::SaberRules::{DuelAdvanced as Mda, Ja};
    let hand = |rules, lit, lightsaber| InHand {
        rules,
        lit,
        lightsaber,
        own: true,
    };
    let rows = [
        (
            "MDA, lit lightsaber",
            hand(Mda, true, true),
            true,
            (false, true),
        ),
        ("MDA, blaster", hand(Mda, false, false), true, (true, false)),
        (
            "MDA, vibroblade",
            hand(Mda, true, false),
            true,
            (true, false),
        ),
        (
            "MDA, unlit lightsaber",
            hand(Mda, false, true),
            true,
            (true, false),
        ),
        (
            "JA, lit lightsaber",
            hand(Ja, true, true),
            true,
            (true, false),
        ),
        (
            "MDA, Block rebound",
            hand(Mda, true, true),
            false,
            (true, true),
        ),
        (
            "JA, Block rebound",
            hand(Ja, true, true),
            false,
            (true, true),
        ),
        (
            "MDA, own lit lightsaber, moving a companion",
            InHand {
                own: false,
                ..hand(Mda, true, true)
            },
            true,
            (true, false),
        ),
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
