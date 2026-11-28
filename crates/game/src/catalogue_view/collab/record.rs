//! A tryout recorded as story steps. The owner's keys, clicks, wheel and
//! typing on the live screen become the steps `Staged::step` takes, each
//! resolved from the control that has the focus or that was clicked, never
//! from a pixel.

use std::path::Path;

use fmt_odyssey::gui::Kind;
use ui::catalogue::Step;
use ui::draw::{DrawList, Mark};
use ui::input::{Key, UiInput};
use ui::screen::FocusLink;

use super::super::Catalogue;

#[derive(Default)]
pub struct Recorder {
    /// The steps taken since the story opened, in order.
    pub steps: Vec<Step>,
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
    pub fn before(&mut self, input: &UiInput, frame: &Frame<'_>) -> Vec<Step> {
        let mut made = Vec::new();
        if !input.text.is_empty() {
            made.push(Step::Type(input.text.clone()));
        }
        for key in &input.keys {
            if !ui::catalogue::MOVES.iter().any(|(_, known)| known == key) {
                continue;
            }
            match (key, frame.focused) {
                (Key::Activate, Some(tag)) => {
                    made.push(Step::Press(tag.to_string()));
                    self.hovered = None;
                }
                _ => {
                    made.extend(self.hover_step());
                    made.push(Step::Key(*key));
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
                    _ => made.push(Step::Press(mark.tag.clone())),
                }
                self.hovered = None;
                clicked = true;
            }
        }
        if input.keys.is_empty() && input.text.is_empty() {
            for name in &input.raw {
                let left_click = name == LEFT_CLICK && (input.click || clicked);
                if !left_click && !name.starts_with(RELEASED_RAW) {
                    made.push(Step::Raw(name.clone()));
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
                made.push(Step::Hover(mark.tag.clone()));
            }
            made.push(Step::Scroll(-input.wheel));
        }
        self.steps.extend(made.iter().cloned());
        made
    }

    /// The row of the list the frame's click landed on, as the step that
    /// replays it: `row TAG N`.
    pub fn row_step(&mut self, row: impl FnOnce(&str, usize) -> Option<usize>) -> Option<Step> {
        let (tag, at) = self.clicked_list.take()?;
        let row = row(&tag, at)?;
        let step = Step::Row { tag, row };
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

    fn hover_step(&mut self) -> Option<Step> {
        self.hovered.take().map(Step::Hover)
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

/// A number that is the same for two frames when `signature` is, which
/// takes no text made for each control: it is read every frame to know
/// whether the control tree is out of date. `extra` is anything else the
/// frame depends on.
pub fn fingerprint(layers: &[DrawList], extra: (&str, usize)) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    extra.hash(&mut hasher);
    for list in layers {
        list.marks.len().hash(&mut hasher);
        for mark in &list.marks {
            mark.tag.hash(&mut hasher);
            mark.rect.map(|edge| edge.round() as i64).hash(&mut hasher);
            (mark.focused, mark.disabled).hash(&mut hasher);
            for words in &mark.words {
                words.bytes.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// The text of a story that starts as `from` does and then takes `steps`,
/// to append to the story file.
pub fn story_text(name: &str, from: &ui::catalogue::Story, steps: &[Step]) -> String {
    let quoted = |text: &str| {
        let mut out = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                control if control.is_control() => {
                    out.push_str(&format!("\\u{:04X}", u32::from(control)))
                }
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
    if from.games != ui::catalogue::GAMES {
        let games: Vec<String> = from.games.iter().map(|game| quoted(game.key())).collect();
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
                .map(|(game, name)| format!("{} = {}", game.key(), quoted(name)))
                .collect();
            text += &format!("{field} = {{ {} }}\n", each.join(", "));
        }
    }
    let all: Vec<&Step> = from.steps.iter().chain(steps).collect();
    if !all.is_empty() {
        text += "steps = [\n";
        for step in all {
            text += &format!("  {},\n", quoted(&step.to_string()));
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
pub fn replays(cat: &Catalogue, steps: &[Step]) -> Result<(), String> {
    let start = &cat.start;
    let capture = start
        .story
        .capture_for(start.game, ui::catalogue::Input::Mouse)
        .ok_or("no capture for the mouse")?;
    let scene = crate::story_stage::Scene {
        game: start.game,
        screen: start.story.screen.clone(),
        steps: steps.to_vec(),
        error: start.story.error.clone(),
        capture: capture.map(str::to_string),
    };
    let mut staged = crate::story_stage::stage(&cat.discovery, scene, cat.size())?;
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
