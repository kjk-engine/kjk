//! The probes of the bindings (`kjk sim probes`, group "bindings").

use super::*;
use sim::probe::check;

pub(super) fn checks() -> sim::probe::Checks {
    vec![
        ("input names", input_names()),
        ("reversed mouse", reversed_mouse()),
        ("bindings refused", refused()),
        ("pad reset", pad_reset()),
        ("pad rows", pad_rows()),
    ]
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
/// right button attacks, the left is the other mode, the old switch gone.
fn reversed_mouse() -> Result<String, String> {
    let mut read: BTreeMap<String, String> = [("keys.layout", "2"), ("mouse.reverse", "true")]
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .into_iter()
        .collect();
    let changed = relayout(&mut read);
    let mut settings = Settings::defaults();
    settings.values.extend(read.clone());
    let keys = settings.keys();
    let (attack, alt) = (keys.input(Action::Attack), keys.input(Action::AltAttack));
    check(
        changed
            && attack == Input::Mouse(MouseButton::Right)
            && alt == Input::Mouse(MouseButton::Left)
            && !read.contains_key("mouse.reverse"),
        format!("attack {}, alt {}, {read:?}", attack.name(), alt.name()),
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
