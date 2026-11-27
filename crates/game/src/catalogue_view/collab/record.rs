//! A tryout recorded as story steps. The owner's keys, clicks, wheel and
//! typing on the live screen become the steps `Staged::step` takes, each
//! resolved from the control that has the focus or that was clicked, never
//! from a pixel.

use std::path::Path;

use fmt_odyssey::gui::Kind;
use ui::draw::{DrawList, Mark};
use ui::input::{Key, UiInput};
use ui::screen::FocusLink;

use super::super::Catalogue;

#[derive(Default)]
pub struct Recorder {
    /// The steps taken since the story opened, in order.
    pub steps: Vec<String>,
    /// The control the pointer moved the focus to, until a step needs it.
    hovered: Option<String>,
    /// The list a click this frame landed on, and the place of its screen
    /// in the stack. The click may open a screen over that one, so the row
    /// is read from that screen once the click is taken, as the cursor the
    /// click moved.
    pub clicked_list: Option<(String, usize)>,
}

/// What the recorder needs to know of the frame the input lands on.
pub struct Frame<'a> {
    /// The screens as the owner saw them, bottom first.
    pub layers: &'a [DrawList],
    pub links: &'a [FocusLink],
    pub focused: Option<&'a str>,
    pub window: (f32, f32),
    /// The place of the top screen in the stack, from the bottom.
    pub depth: usize,
}

/// The name `UiInput::raw` gives the main mouse button, and the prefix of a
/// key let go. Neither is a press to replay.
const LEFT_CLICK: &str = "MouseLeft";
use ui::screens::key_mapping::RELEASED_RAW;

/// The key steps and the keys that make them.
const KEYS: [(Key, &str); 13] = [
    (Key::Up, "Up"),
    (Key::Down, "Down"),
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Activate, "Activate"),
    (Key::Back, "Back"),
    (Key::Primary, "Primary"),
    (Key::Second, "Second"),
    (Key::Accept, "Accept"),
    (Key::NextTab, "NextTab"),
    (Key::PrevTab, "PrevTab"),
    (Key::NextPage, "NextPage"),
    (Key::PrevPage, "PrevPage"),
];

/// The controls of `list` under `point` (logical window pixels), the one
/// painted last first.
pub fn under(list: &DrawList, window: (f32, f32), point: [f32; 2]) -> impl Iterator<Item = &Mark> {
    let (scale, [x, y]) = list.placed(window);
    let at = [(point[0] - x) / scale, (point[1] - y) / scale];
    list.marks.iter().rev().filter(move |mark| {
        let [left, top, width, height] = mark.rect;
        !mark.tag.is_empty()
            && at[0] >= left
            && at[0] < left + width
            && at[1] >= top
            && at[1] < top + height
    })
}

impl Recorder {
    /// The steps the input of one frame makes, before the screens take it.
    pub fn before(&mut self, input: &UiInput, frame: &Frame<'_>) -> Vec<String> {
        let mut made = Vec::new();
        if !input.text.is_empty() {
            made.push(format!("type {}", input.text));
        }
        for key in &input.keys {
            let Some((_, name)) = KEYS.iter().find(|(known, _)| known == key) else {
                continue;
            };
            match (key, frame.focused) {
                (Key::Activate, Some(tag)) => {
                    made.push(format!("press {tag}"));
                    self.hovered = None;
                }
                _ => {
                    made.extend(self.hover_step());
                    made.push(format!("key {name}"));
                }
            }
        }
        let top = frame.layers.last();
        let point = input.pointer;
        let mut clicked = false;
        if input.click
            && let (Some(list), Some(point)) = (top, point)
        {
            let linked = |mark: &&Mark| frame.links.iter().any(|(tag, _)| *tag == mark.tag);
            if let Some(mark) = under(list, frame.window, point).find(linked) {
                match mark.kind {
                    Kind::ListBox => self.clicked_list = Some((mark.tag.clone(), frame.depth)),
                    _ => made.push(format!("press {}", mark.tag)),
                }
                self.hovered = None;
                clicked = true;
            }
        }
        if input.keys.is_empty() && input.text.is_empty() {
            for name in &input.raw {
                let left_click = name == LEFT_CLICK && (input.click || clicked);
                if !left_click && !name.starts_with(RELEASED_RAW) {
                    made.push(format!("raw {name}"));
                }
            }
        }
        if input.wheel != 0.0 {
            made.extend(self.hover_step());
            if let (Some(list), Some(point)) = (top, point)
                && let Some(mark) = under(list, frame.window, point)
                    .find(|mark| frame.links.iter().any(|(tag, _)| *tag == mark.tag))
                    .or_else(|| under(list, frame.window, point).next())
            {
                made.push(format!("hover {}", mark.tag));
            }
            made.push(format!("scroll {}", -input.wheel));
        }
        self.steps.extend(made.iter().cloned());
        made
    }

    /// The row of the list the frame's click landed on, as the step that
    /// replays it: `row TAG N`.
    pub fn row_step(&mut self, row: impl FnOnce(&str, usize) -> Option<usize>) -> Option<String> {
        let (tag, at) = self.clicked_list.take()?;
        let step = format!("row {tag} {}", row(&tag, at)?);
        self.steps.push(step.clone());
        Some(step)
    }

    /// What the frame left: a focus the pointer moved is remembered, to be
    /// recorded as a hover if a later step depends on it.
    pub fn after(&mut self, input: &UiInput, before: Option<&str>, now: Option<&str>) {
        if input.moved && input.keys.is_empty() && !input.click && before != now {
            self.hovered = now.map(str::to_string);
        }
    }

    fn hover_step(&mut self) -> Option<String> {
        self.hovered.take().map(|tag| format!("hover {tag}"))
    }

    pub fn clear(&mut self) {
        *self = Recorder::default();
    }
}

/// What a replay of the recorded steps must show again: each control's tag,
/// place, focus and words, as the screens drew them. The words are in it so
/// that a change of text alone, a credit count, counts as a change.
pub fn signature(layers: &[DrawList]) -> Vec<String> {
    let mut lines = Vec::new();
    for list in layers {
        for mark in &list.marks {
            let [left, top, width, height] = mark.rect.map(f32::round);
            let words: Vec<_> = (mark.words.iter())
                .map(|words| String::from_utf8_lossy(&words.bytes))
                .collect();
            lines.push(format!(
                "{} {left} {top} {width} {height} {} {} {words:?}",
                mark.tag, mark.focused, mark.disabled
            ));
        }
    }
    lines
}

/// The text of a story that starts as `from` does and then takes `steps`,
/// to append to the story file.
pub fn story_text(name: &str, from: &ui::catalogue::Story, steps: &[String]) -> String {
    let quoted = |text: &str| {
        let mut out = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                other => out.push(other),
            }
        }
        out.push('"');
        out
    };
    let mut text = format!(
        "\n[[story]]\nname = {}\nscreen = {}\n",
        quoted(name),
        quoted(&from.screen)
    );
    if from.games != ["k1", "tsl"] {
        let games: Vec<String> = from.games.iter().map(|game| quoted(game)).collect();
        text += &format!("games = [{}]\n", games.join(", "));
    }
    if let Some(error) = &from.error {
        text += &format!("error = {}\n", quoted(error));
    }
    for (field, captures) in [
        ("capture", &from.capture),
        ("pad_capture", &from.pad_capture),
    ] {
        if !captures.is_empty() {
            let each: Vec<String> = (captures.iter())
                .map(|(game, name)| format!("{game} = {}", quoted(name)))
                .collect();
            text += &format!("{field} = {{ {} }}\n", each.join(", "));
        }
    }
    let all: Vec<&String> = from.steps.iter().chain(steps).collect();
    if !all.is_empty() {
        text += "steps = [\n";
        for step in all {
            text += &format!("  {},\n", quoted(step));
        }
        text += "]\n";
    }
    text
}

/// `text` added to the end of the story file at `path`.
pub fn append(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// A story's name that no story of `taken` has, `base` or `base (2)`, …
pub fn free_name(base: &str, taken: &[String]) -> String {
    let mut name = base.to_string();
    let mut n = 1;
    while taken.contains(&name) {
        n += 1;
        name = format!("{base} ({n})");
    }
    name
}

/// The moment as a story's default name carries it: `Tryout 2026-11-27
/// 21:04`, in UTC.
pub fn tryout_name() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = saves::civil_from_days((seconds / 86_400) as i64);
    let rest = seconds % 86_400;
    format!(
        "Tryout {year}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60
    )
}

/// Whether a story staged anew from `steps` shows what `cat` shows now.
pub fn replays(cat: &Catalogue, steps: &[String]) -> Result<(), String> {
    let start = &cat.start;
    let size = cat.size();
    let capture = start
        .story
        .capture_for(start.game.key(), ui::catalogue::Input::Mouse)
        .ok_or("no capture for the mouse")?;
    let mut staged = crate::story_stage::stage(
        &cat.discovery,
        (start.game, &start.story.screen),
        (steps, start.story.error.clone(), capture),
        (size.pixels, size.factor),
    )?;
    let shown: Vec<DrawList> = staged.frame().into_iter().map(|shown| shown.list).collect();
    let (replayed, live) = (signature(&shown), signature(&cat.layers));
    match replayed == live {
        true => Ok(()),
        false => {
            let first = replayed.iter().zip(&live).position(|(a, b)| a != b);
            Err(format!(
                "the replay differs: {} controls against {}, the first difference at {:?}",
                replayed.len(),
                live.len(),
                first.map(|at| (replayed[at].clone(), live[at].clone()))
            ))
        }
    }
}
