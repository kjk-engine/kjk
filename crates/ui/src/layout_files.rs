//! kjk's own layout files (docs/UI.md, "Layout files"). A file is either an
//! override of a KOTOR layout or a standalone layout, and both are read into
//! the same `Gui` that a `.gui` is read into, so the stack, the painter and
//! the catalogue treat them alike.
//!
//! A file names KOTOR's resources by name only (textures, fonts, the parts it
//! copies) and holds no game art and no game text. Words are either kjk's
//! own or a talk table line given by its number.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fmt_odyssey::gui::{Border, Control, Gui, Kind, MoveTo, Rect, Text, align};
use toml_edit::{DocumentMut, Item, TableLike};

/// The folder of the layout files, from the repository's root.
pub const LAYOUTS: &str = "data/ui/layouts";

/// The prefix of the name a screen gives its layout when the layout is a
/// standalone file: `kjk:test_screen` is `data/ui/layouts/test_screen.toml`.
pub const PREFIX: &str = "kjk:";

/// The setting that turns the overrides of KOTOR's layouts on (`true`) and
/// off (`false`, and when it is not set).
pub const OVERRIDES: &str = "ui.layout_overrides";

/// The frame KOTOR 2's full-screen layouts are made for, which a standalone
/// layout has unless it says otherwise.
const FRAME: [i32; 2] = [800, 600];

/// The layout files' folder in the checkout this was built from.
pub fn folder() -> PathBuf {
    crate::catalogue::root().join(LAYOUTS)
}

/// The file that overrides KOTOR's layout `name`, if there is one.
pub fn override_path(name: &str) -> PathBuf {
    folder().join(format!("{}.toml", name.to_ascii_lowercase()))
}

/// The file of the standalone layout `name` (written with its `kjk:`
/// prefix), if the name has one.
pub fn standalone_path(name: &str) -> Option<PathBuf> {
    let file = name.strip_prefix(PREFIX)?;
    let plain = file
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || letter == '_' || letter == '-');
    plain.then(|| folder().join(format!("{file}.toml")))
}

/// The names that `data/ui/tokens.json` gives colours and fonts.
#[derive(Debug, Clone, Default)]
pub struct Tokens {
    colours: BTreeMap<String, [f32; 3]>,
    fonts: BTreeMap<String, String>,
}

impl Tokens {
    /// The tokens of the checkout, or the copy this was built with where
    /// the file is not there.
    pub fn read() -> Result<Tokens, String> {
        let path = crate::catalogue::root().join("data/ui/tokens.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| include_str!("../../../data/ui/tokens.json").to_string());
        Tokens::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Tokens, String> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|error| format!("tokens.json: {error}"))?;
        let mut tokens = Tokens::default();
        walk(&json["color"], "color", &mut |name, value| {
            let components = value["$value"]["components"].as_array()?;
            let get = |i: usize| components.get(i)?.as_f64().map(|c| c as f32);
            tokens
                .colours
                .insert(name.to_string(), [get(0)?, get(1)?, get(2)?]);
            Some(())
        });
        walk(&json["font"], "font", &mut |name, value| {
            tokens
                .fonts
                .insert(name.to_string(), value["$value"].as_str()?.to_string());
            Some(())
        });
        Ok(tokens)
    }

    /// A colour as a file writes it: a token (`color.tsl-layout.base`), a
    /// hex colour (`#1ab28c`) or three numbers from 0 to 1.
    fn colour(&self, item: &Item) -> Result<[f32; 3], String> {
        if let Some(array) = item.as_array() {
            let numbers: Vec<f32> = array
                .iter()
                .filter_map(|value| value.as_float().or(value.as_integer().map(|n| n as f64)))
                .map(|value| value as f32)
                .collect();
            return match numbers[..] {
                [r, g, b] if array.len() == 3 => Ok([r, g, b]),
                _ => Err("a colour in numbers is three of them, [r, g, b], from 0 to 1".into()),
            };
        }
        let name = item
            .as_str()
            .ok_or("a colour is a token, a hex colour or three numbers")?;
        if let Some(hex) = name.strip_prefix('#') {
            let bytes = (hex.len() == 6)
                .then(|| u32::from_str_radix(hex, 16).ok())
                .flatten()
                .ok_or(format!("{name:?} is not a hex colour like #1ab28c"))?;
            return Ok([16, 8, 0].map(|shift| ((bytes >> shift) & 0xff) as f32 / 255.0));
        }
        self.colours.get(name).copied().ok_or_else(|| {
            format!(
                "unknown colour token {name:?}; the tokens are {}",
                names(self.colours.keys())
            )
        })
    }

    /// A font as a file writes it: a token (`font.body`) or the name of
    /// KOTOR's font resource (`dialogfont16x16`).
    fn font(&self, name: &str) -> Result<String, String> {
        if name.starts_with("font.") {
            return self.fonts.get(name).cloned().ok_or_else(|| {
                format!(
                    "unknown font token {name:?}; the tokens are {}",
                    names(self.fonts.keys())
                )
            });
        }
        Ok(name.to_ascii_lowercase())
    }
}

fn walk(
    at: &serde_json::Value,
    path: &str,
    leaf: &mut dyn FnMut(&str, &serde_json::Value) -> Option<()>,
) {
    let Some(map) = at.as_object() else {
        return;
    };
    for (key, value) in map {
        if key.starts_with('$') {
            continue;
        }
        let name = format!("{path}.{key}");
        if value.get("$value").is_some() {
            leaf(&name, value);
        } else {
            walk(value, &name, leaf);
        }
    }
}

fn names<'a>(names: impl Iterator<Item = &'a String>) -> String {
    names.cloned().collect::<Vec<_>>().join(", ")
}

/// What a control's words are: kjk's own, or a talk table line.
#[derive(Debug, Clone, PartialEq)]
pub enum Words {
    Own(String),
    Strref(u32),
}

/// A frame that a control is drawn with, as the file gives it: the fields it
/// writes, which replace those of the frame it changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BorderSpec {
    pub corner: Option<String>,
    pub edge: Option<String>,
    pub fill: Option<String>,
    pub fill_style: Option<i32>,
    pub dimension: Option<i32>,
    pub inner_offset: Option<i32>,
    pub color: Option<[f32; 3]>,
    pub pulsing: Option<bool>,
}

/// The four ways a control's frame can be drawn.
const BORDERS: [&str; 4] = ["border", "hilight", "selected", "hilight_selected"];

/// One control in a file: the fields it writes. A field it leaves out stays
/// as the KOTOR control has it (an override) or as the control's part has
/// it (a standalone layout).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spec {
    pub tag: String,
    pub kind: Option<Kind>,
    /// `LAYOUT:TAG`, the KOTOR control this one starts as a copy of.
    pub part: Option<(String, String)>,
    pub parent: Option<String>,
    /// Left, top, width and height, in the layout's pixels.
    pub rect: [Option<i32>; 4],
    pub remove: bool,
    pub words: Option<Words>,
    pub font: Option<String>,
    pub align: Option<i32>,
    pub text_color: Option<[f32; 3]>,
    pub borders: [Option<BorderSpec>; 4],
    /// Up, down, left and right, as tags (`None`: no control).
    pub move_to: Option<[Option<Option<String>>; 4]>,
    pub looping: Option<bool>,
    pub padding: Option<i32>,
}

/// What a standalone layout's root is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RootSpec {
    pub frame: [i32; 2],
    /// `LAYOUT`, the KOTOR layout whose root's frame and colour this one
    /// starts as a copy of.
    pub like: Option<String>,
    pub border: Option<BorderSpec>,
    pub color: Option<[f32; 3]>,
}

/// A layout file, read.
#[derive(Debug, Clone, PartialEq)]
pub enum File {
    /// Changes to KOTOR's layout `base`.
    Override { base: String, controls: Vec<Spec> },
    /// A layout with no KOTOR base.
    Standalone {
        name: String,
        root: RootSpec,
        controls: Vec<Spec>,
    },
}

const CONTROL_FIELDS: [&str; 16] = [
    "tag",
    "kind",
    "part",
    "parent",
    "left",
    "top",
    "width",
    "height",
    "remove",
    "text",
    "strref",
    "font",
    "align",
    "text_color",
    "move_to",
    "looping",
];
const MORE_CONTROL_FIELDS: [&str; 5] = [
    "padding",
    "border",
    "hilight",
    "selected",
    "hilight_selected",
];
const BORDER_FIELDS: [&str; 8] = [
    "corner",
    "edge",
    "fill",
    "fill_style",
    "dimension",
    "inner_offset",
    "color",
    "pulsing",
];
const KINDS: [(&str, Kind); 8] = [
    ("panel", Kind::Panel),
    ("label", Kind::Label),
    ("button", Kind::Button),
    ("check_box", Kind::CheckBox),
    ("slider", Kind::Slider),
    ("scroll_bar", Kind::ScrollBar),
    ("progress", Kind::Progress),
    ("list_box", Kind::ListBox),
];

/// The tag a kind is written with.
pub fn kind_name(kind: Kind) -> Option<&'static str> {
    KINDS
        .iter()
        .find(|(_, known)| *known == kind)
        .map(|(name, _)| *name)
}

/// Reads fields of one table, naming the file and the table in every error.
struct Reader<'a> {
    file: &'a str,
    what: String,
    table: &'a dyn TableLike,
}

impl<'a> Reader<'a> {
    fn fail(&self, why: impl std::fmt::Display) -> String {
        format!("{}: {}: {why}", self.file, self.what)
    }

    fn known(&self, fields: &[&[&str]]) -> Result<(), String> {
        let known: Vec<&str> = fields
            .iter()
            .flat_map(|group| group.iter().copied())
            .collect();
        match self.table.iter().find(|(key, _)| !known.contains(key)) {
            Some((key, _)) => Err(self.fail(format!(
                "unknown field `{key}`; the fields are {}",
                known.join(", ")
            ))),
            None => Ok(()),
        }
    }

    fn item(&self, key: &str) -> Option<&'a Item> {
        self.table.get(key)
    }

    fn text(&self, key: &str) -> Result<Option<String>, String> {
        match self.item(key) {
            None => Ok(None),
            Some(item) => item
                .as_str()
                .map(|text| Some(text.to_string()))
                .ok_or(self.fail(format!("`{key}` is not text"))),
        }
    }

    fn int(&self, key: &str) -> Result<Option<i32>, String> {
        match self.item(key) {
            None => Ok(None),
            Some(item) => item
                .as_integer()
                .and_then(|n| i32::try_from(n).ok())
                .map(Some)
                .ok_or(self.fail(format!("`{key}` is not a whole number"))),
        }
    }

    fn flag(&self, key: &str) -> Result<Option<bool>, String> {
        match self.item(key) {
            None => Ok(None),
            Some(item) => item
                .as_bool()
                .map(Some)
                .ok_or(self.fail(format!("`{key}` is not true or false"))),
        }
    }

    fn colour(&self, key: &str, tokens: &Tokens) -> Result<Option<[f32; 3]>, String> {
        match self.item(key) {
            None => Ok(None),
            Some(item) => tokens
                .colour(item)
                .map(Some)
                .map_err(|why| self.fail(format!("`{key}`: {why}"))),
        }
    }

    fn sub(&self, key: &str) -> Result<Option<Reader<'a>>, String> {
        match self.item(key) {
            None => Ok(None),
            Some(item) => {
                let table = item
                    .as_table_like()
                    .ok_or(self.fail(format!("`{key}` is not a table")))?;
                Ok(Some(Reader {
                    file: self.file,
                    what: format!("{}, {key}", self.what),
                    table,
                }))
            }
        }
    }
}

fn border_spec(reader: &Reader, tokens: &Tokens) -> Result<BorderSpec, String> {
    reader.known(&[&BORDER_FIELDS])?;
    Ok(BorderSpec {
        corner: reader.text("corner")?,
        edge: reader.text("edge")?,
        fill: reader.text("fill")?,
        fill_style: reader.int("fill_style")?,
        dimension: reader.int("dimension")?,
        inner_offset: reader.int("inner_offset")?,
        color: reader.colour("color", tokens)?,
        pulsing: reader.flag("pulsing")?,
    })
}

fn part(reader: &Reader, key: &str) -> Result<Option<(String, String)>, String> {
    let Some(text) = reader.text(key)? else {
        return Ok(None);
    };
    match text.split_once(':') {
        Some((layout, tag)) if !layout.is_empty() && !tag.is_empty() => {
            Ok(Some((layout.to_string(), tag.to_string())))
        }
        _ => Err(reader.fail(format!(
            "`{key}` is {text:?}, and a part is written LAYOUT:TAG, as optionsmain:BTN_BACK"
        ))),
    }
}

fn alignment(reader: &Reader) -> Result<Option<i32>, String> {
    let Some(words) = reader.text("align")? else {
        return Ok(None);
    };
    let mut bits = 0;
    for word in words.split_whitespace() {
        bits |= match word {
            "left" => align::LEFT,
            "centre" | "center" => align::CENTRE,
            "right" => align::RIGHT,
            "top" => align::TOP,
            "middle" => align::MIDDLE,
            "bottom" => align::BOTTOM,
            other => {
                return Err(reader.fail(format!(
                    "`align` has {other:?}; the words are left, centre, right, top, middle and bottom"
                )));
            }
        };
    }
    Ok(Some(bits))
}

fn move_to(reader: &Reader) -> Result<Option<[Option<Option<String>>; 4]>, String> {
    let Some(links) = reader.sub("move_to")? else {
        return Ok(None);
    };
    links.known(&[&["up", "down", "left", "right"]])?;
    let mut out: [Option<Option<String>>; 4] = Default::default();
    for (at, key) in ["up", "down", "left", "right"].into_iter().enumerate() {
        out[at] = links.text(key)?.map(|tag| (tag != "none").then_some(tag));
    }
    Ok(Some(out))
}

fn control_spec(reader: &Reader, tokens: &Tokens) -> Result<Spec, String> {
    reader.known(&[&CONTROL_FIELDS, &MORE_CONTROL_FIELDS])?;
    let tag = reader
        .text("tag")?
        .ok_or(reader.fail("a control has no `tag`"))?;
    let kind = match reader.text("kind")? {
        None => None,
        Some(name) => Some(
            KINDS
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, kind)| *kind)
                .ok_or_else(|| {
                    reader.fail(format!(
                        "kind {name:?} is none of {}",
                        KINDS.map(|(name, _)| name).join(", ")
                    ))
                })?,
        ),
    };
    let words = match (reader.text("text")?, reader.int("strref")?) {
        (Some(_), Some(_)) => return Err(reader.fail("`text` and `strref` are both given")),
        (Some(own), None) => Some(Words::Own(own)),
        (None, Some(n)) => Some(Words::Strref(
            u32::try_from(n).map_err(|_| reader.fail("`strref` is below 0"))?,
        )),
        (None, None) => None,
    };
    let mut borders: [Option<BorderSpec>; 4] = Default::default();
    for (at, key) in BORDERS.into_iter().enumerate() {
        if let Some(inner) = reader.sub(key)? {
            borders[at] = Some(border_spec(&inner, tokens)?);
        }
    }
    let font = match reader.text("font")? {
        Some(name) => Some(tokens.font(&name).map_err(|why| reader.fail(why))?),
        None => None,
    };
    Ok(Spec {
        kind,
        part: part(reader, "part")?,
        parent: reader.text("parent")?,
        rect: [
            reader.int("left")?,
            reader.int("top")?,
            reader.int("width")?,
            reader.int("height")?,
        ],
        remove: reader.flag("remove")?.unwrap_or(false),
        words,
        font,
        align: alignment(reader)?,
        text_color: reader.colour("text_color", tokens)?,
        borders,
        move_to: move_to(reader)?,
        looping: reader.flag("looping")?,
        padding: reader.int("padding")?,
        tag,
    })
}

fn controls(document: &DocumentMut, file: &str, tokens: &Tokens) -> Result<Vec<Spec>, String> {
    let Some(item) = document.get("control") else {
        return Ok(Vec::new());
    };
    let tables = item
        .as_array_of_tables()
        .ok_or(format!("{file}: `control` is not written [[control]]"))?;
    let mut specs: Vec<Spec> = Vec::new();
    for (at, table) in tables.iter().enumerate() {
        let tag = table.get("tag").and_then(Item::as_str).unwrap_or("");
        let reader = Reader {
            file,
            what: if tag.is_empty() {
                format!("control {}", at + 1)
            } else {
                format!("control {tag}")
            },
            table,
        };
        let spec = control_spec(&reader, tokens)?;
        if specs.iter().any(|known| known.tag == spec.tag) {
            return Err(reader.fail("a second control has this tag"));
        }
        specs.push(spec);
    }
    Ok(specs)
}

/// The layout file `text`, named `file` in messages.
pub fn parse(file: &str, text: &str, tokens: &Tokens) -> Result<File, String> {
    let document: DocumentMut = text.parse().map_err(|error| format!("{file}: {error}"))?;
    let top = Reader {
        file,
        what: "the file".to_string(),
        table: document.as_table(),
    };
    top.known(&[&["override", "layout", "frame", "root", "control"]])?;
    let controls = controls(&document, file, tokens)?;
    match (top.text("override")?, top.text("layout")?) {
        (Some(_), Some(_)) => Err(top.fail("`override` and `layout` are both given")),
        (None, None) => Err(top.fail(
            "it has neither `override = \"KOTOR LAYOUT\"` nor `layout = \"NAME\"` at its top",
        )),
        (Some(base), None) => {
            if top.item("root").is_some() || top.item("frame").is_some() {
                return Err(top.fail("an override has no `root` or `frame`"));
            }
            Ok(File::Override { base, controls })
        }
        (None, Some(name)) => {
            let mut root = RootSpec {
                frame: FRAME,
                ..RootSpec::default()
            };
            if let Some(frame) = top.item("frame") {
                let numbers: Vec<i32> = frame
                    .as_array()
                    .map(|array| {
                        array
                            .iter()
                            .filter_map(|n| n.as_integer().and_then(|n| i32::try_from(n).ok()))
                            .collect()
                    })
                    .unwrap_or_else(Vec::new);
                root.frame = match numbers[..] {
                    [width, height] if width > 0 && height > 0 => [width, height],
                    _ => return Err(top.fail("`frame` is [width, height], as [800, 600]")),
                };
            }
            if let Some(inner) = top.sub("root")? {
                inner.known(&[&["like", "border", "color"]])?;
                root.like = inner.text("like")?;
                root.color = inner.colour("color", tokens)?;
                if let Some(border) = inner.sub("border")? {
                    root.border = Some(border_spec(&border, tokens)?);
                }
            }
            Ok(File::Standalone {
                name,
                root,
                controls,
            })
        }
    }
}

/// The KOTOR layouts a file copies parts or its root from, read by name.
pub type Parts<'a> = dyn FnMut(&str) -> Result<Gui, String> + 'a;

fn border_of(base: Option<&Border>, spec: &BorderSpec) -> Border {
    let mut border = base.cloned().unwrap_or_else(Border::default);
    if let Some(corner) = &spec.corner {
        border.corner = corner.to_ascii_lowercase();
    }
    if let Some(edge) = &spec.edge {
        border.edge = edge.to_ascii_lowercase();
    }
    if let Some(fill) = &spec.fill {
        border.fill = fill.to_ascii_lowercase();
    }
    border.fill_style = spec.fill_style.unwrap_or(border.fill_style);
    border.dimension = spec.dimension.unwrap_or(border.dimension);
    border.inner_offset = spec.inner_offset.unwrap_or(border.inner_offset);
    border.color = spec.color.or(border.color);
    border.pulsing = spec.pulsing.unwrap_or(border.pulsing);
    border
}

fn shift(control: &mut Control, dx: i32, dy: i32) {
    control.extent.left += dx;
    control.extent.top += dy;
    for inner in control
        .children
        .iter_mut()
        .chain(control.proto.as_deref_mut())
        .chain(control.scrollbar.as_deref_mut())
    {
        shift(inner, dx, dy);
    }
}

fn find_mut<'g>(controls: &'g mut [Control], tag: &str) -> Option<&'g mut Control> {
    for control in controls {
        if control.tag.eq_ignore_ascii_case(tag) {
            return Some(control);
        }
        if let Some(found) = find_mut(&mut control.children, tag) {
            return Some(found);
        }
    }
    None
}

/// Takes the control `tag` out of `controls` (or out of one of their
/// children), and returns its id.
fn take(controls: &mut Vec<Control>, tag: &str) -> Option<i32> {
    if let Some(at) = controls
        .iter()
        .position(|control| control.tag.eq_ignore_ascii_case(tag))
    {
        return Some(controls.remove(at).id);
    }
    controls
        .iter_mut()
        .find_map(|control| take(&mut control.children, tag))
}

fn forget_links(controls: &mut [Control], id: i32) {
    for control in controls {
        if let Some(links) = control.moveto.as_mut() {
            for link in [
                &mut links.up,
                &mut links.down,
                &mut links.left,
                &mut links.right,
            ] {
                if *link == id {
                    *link = -1;
                }
            }
        }
        forget_links(&mut control.children, id);
    }
}

fn renumber(control: &mut Control, next: &mut i32, parent: i32) {
    control.id = *next;
    control.parent = parent;
    *next += 1;
    control.moveto = None;
    let own = control.id;
    for inner in control
        .children
        .iter_mut()
        .chain(control.proto.as_deref_mut())
        .chain(control.scrollbar.as_deref_mut())
    {
        renumber(inner, next, own);
    }
}

fn next_id(controls: &[Control]) -> i32 {
    fn highest(controls: &[Control]) -> i32 {
        controls
            .iter()
            .map(|control| {
                highest(&control.children)
                    .max(control.id)
                    .max(control.proto.as_deref().map_or(-1, |proto| proto.id))
                    .max(control.scrollbar.as_deref().map_or(-1, |bar| bar.id))
            })
            .max()
            .unwrap_or(-1)
    }
    highest(controls) + 1
}

fn tags_of(gui: &Gui) -> String {
    let tags: Vec<&str> = gui.walk().map(|control| control.tag.as_str()).collect();
    let shown = tags.iter().take(24).copied().collect::<Vec<_>>().join(", ");
    if tags.len() > 24 {
        format!("{shown}, and {} more", tags.len() - 24)
    } else {
        shown
    }
}

/// The changes `spec` makes to `control`.
fn change(control: &mut Control, spec: &Spec, words: &dyn Fn(&str) -> Vec<u8>) {
    let [left, top, width, height] = spec.rect;
    let (dx, dy) = (
        left.map_or(0, |left| left - control.extent.left),
        top.map_or(0, |top| top - control.extent.top),
    );
    if dx != 0 || dy != 0 {
        shift(control, dx, dy);
    }
    control.extent.width = width.unwrap_or(control.extent.width);
    control.extent.height = height.unwrap_or(control.extent.height);
    if let Some(kind) = spec.kind {
        control.kind = kind;
    }
    for (at, slot) in [
        &mut control.border,
        &mut control.hilight,
        &mut control.selected,
        &mut control.hilight_selected,
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(border) = &spec.borders[at] {
            *slot = Some(border_of(slot.as_ref(), border));
        }
    }
    if spec.words.is_some()
        || spec.font.is_some()
        || spec.align.is_some()
        || spec.text_color.is_some()
    {
        let text = control.text.get_or_insert_with(|| Text {
            alignment: align::CENTRE | align::MIDDLE,
            ..Text::default()
        });
        match &spec.words {
            Some(Words::Own(own)) => {
                text.text = words(own);
                text.strref = None;
            }
            Some(Words::Strref(n)) => {
                text.text = Vec::new();
                text.strref = Some(*n);
            }
            None => {}
        }
        if let Some(font) = &spec.font {
            text.font = font.clone();
        }
        text.alignment = spec.align.unwrap_or(text.alignment);
        text.color = spec.text_color.or(text.color);
    }
    control.looping = spec.looping.unwrap_or(control.looping);
    control.padding = spec.padding.unwrap_or(control.padding);
}

/// Where a new control goes: the layout's own controls, or the children of
/// the control `parent`.
fn container<'g>(gui: &'g mut Gui, parent: Option<&str>) -> Result<&'g mut Vec<Control>, String> {
    let Some(tag) = parent else {
        return Ok(&mut gui.controls);
    };
    if gui.find(tag).is_none() {
        return Err(format!(
            "its parent {tag:?} is not in the layout; its controls are {}",
            tags_of(gui)
        ));
    }
    Ok(&mut find_mut(&mut gui.controls, tag)
        .expect("it was found")
        .children)
}

struct Context<'a, 'p> {
    file: &'a str,
    tokens_font: String,
    parts: &'a mut Parts<'p>,
    loaded: BTreeMap<String, Gui>,
}

impl Context<'_, '_> {
    fn part(&mut self, (layout, tag): &(String, String)) -> Result<Control, String> {
        if !self.loaded.contains_key(layout) {
            let gui = (self.parts)(layout).map_err(|why| format!("part {layout}:{tag}: {why}"))?;
            self.loaded.insert(layout.clone(), gui);
        }
        let gui = &self.loaded[layout];
        gui.find(tag).cloned().ok_or_else(|| {
            format!(
                "part {layout}:{tag}: the layout has no control {tag}; its controls are {}",
                tags_of(gui)
            )
        })
    }
}

/// Puts the changes and the new controls `specs` give into `gui`.
fn apply_controls(gui: &mut Gui, specs: &[Spec], context: &mut Context) -> Result<(), String> {
    let words = |own: &str| crate::strings::bytes(own);
    let mut links: Vec<(String, [Option<Option<String>>; 4])> = Vec::new();
    let file = context.file;
    for spec in specs {
        let why = |what: String| format!("{file}: control {}: {what}", spec.tag);
        if spec.remove {
            let id = take(&mut gui.controls, &spec.tag).ok_or_else(|| {
                why(format!(
                    "the layout has no such control to remove; its controls are {}",
                    tags_of(gui)
                ))
            })?;
            forget_links(&mut gui.controls, id);
            continue;
        }
        if gui.find(&spec.tag).is_some() {
            if spec.part.is_some() {
                return Err(why(
                    "the layout has this control already, and a `part` is for a new one".into(),
                ));
            }
            let control = find_mut(&mut gui.controls, &spec.tag).expect("it was found");
            change(control, spec, &words);
        } else {
            if spec.kind.is_none() && spec.part.is_none() {
                return Err(why(format!(
                    "no such control, and a new one needs a `kind` or a `part`; the controls are {}",
                    tags_of(gui)
                )));
            }
            let mut control = match &spec.part {
                Some(part) => context.part(part).map_err(&why)?,
                None => Control {
                    parent: -1,
                    ..Control::default()
                },
            };
            let mut next = next_id(&gui.controls);
            renumber(&mut control, &mut next, -1);
            control.tag = spec.tag.clone();
            control.parent = -1;
            if spec.part.is_none() {
                control.extent = Rect {
                    left: 0,
                    top: 0,
                    width: 100,
                    height: 30,
                };
            }
            let rect = spec.rect;
            let at = Rect {
                left: rect[0].unwrap_or(control.extent.left),
                top: rect[1].unwrap_or(control.extent.top),
                ..control.extent
            };
            let (dx, dy) = (at.left - control.extent.left, at.top - control.extent.top);
            shift(&mut control, dx, dy);
            let mut rest = spec.clone();
            rest.rect = [None, None, rect[2], rect[3]];
            if control.text.is_none() && spec.words.is_some() && spec.font.is_none() {
                rest.font = Some(context.tokens_font.clone());
            }
            change(&mut control, &rest, &words);
            container(gui, spec.parent.as_deref())
                .map_err(&why)?
                .push(control);
        }
        if let Some(to) = &spec.move_to {
            links.push((spec.tag.clone(), to.clone()));
        }
    }
    for (tag, to) in links {
        let mut ids = [None; 4];
        for (at, link) in to.iter().enumerate() {
            ids[at] = match link {
                None => None,
                Some(None) => Some(-1),
                Some(Some(other)) => Some(
                    gui.find(other)
                        .map(|control| control.id)
                        .ok_or_else(|| {
                            format!(
                                "{}: control {tag}: move_to leads to {other}, which is not in the layout; its controls are {}",
                                context.file,
                                tags_of(gui)
                            )
                        })?,
                ),
            };
        }
        let control = find_mut(&mut gui.controls, &tag).expect("it was placed");
        let old = control.moveto.unwrap_or(MoveTo {
            up: -1,
            down: -1,
            left: -1,
            right: -1,
        });
        control.moveto = Some(MoveTo {
            up: ids[0].unwrap_or(old.up),
            down: ids[1].unwrap_or(old.down),
            left: ids[2].unwrap_or(old.left),
            right: ids[3].unwrap_or(old.right),
        });
    }
    Ok(())
}

/// The override `file` applied to `gui`, KOTOR's layout.
pub fn apply_override(
    gui: &mut Gui,
    layout: &str,
    name: &str,
    file: &File,
    tokens: &Tokens,
    parts: &mut Parts,
) -> Result<(), String> {
    let File::Override { base, controls } = file else {
        return Err(format!("{name}: a standalone layout, not an override"));
    };
    if !base.eq_ignore_ascii_case(layout) {
        return Err(format!(
            "{name}: it overrides {base}, and a file is named for the layout it overrides ({layout})"
        ));
    }
    let mut context = Context {
        file: name,
        tokens_font: tokens.font("font.body")?,
        parts,
        loaded: BTreeMap::new(),
    };
    apply_controls(gui, controls, &mut context)
}

/// The layout a standalone `file` builds.
pub fn build(name: &str, file: &File, tokens: &Tokens, parts: &mut Parts) -> Result<Gui, String> {
    let File::Standalone {
        name: layout,
        root,
        controls,
    } = file
    else {
        return Err(format!("{name}: an override, not a standalone layout"));
    };
    let mut gui = Gui {
        tag: layout.clone(),
        extent: Rect {
            left: 0,
            top: 0,
            width: root.frame[0],
            height: root.frame[1],
        },
        border: None,
        color: root.color,
        alpha: None,
        controls: Vec::new(),
        unread: Vec::new(),
    };
    let mut context = Context {
        file: name,
        tokens_font: tokens.font("font.body")?,
        parts,
        loaded: BTreeMap::new(),
    };
    if let Some(like) = &root.like {
        let base =
            (context.parts)(like).map_err(|why| format!("{name}: root like {like}: {why}"))?;
        gui.border = base.border.clone();
        gui.color = root.color.or(base.color);
        gui.alpha = base.alpha;
    }
    if let Some(border) = &root.border {
        gui.border = Some(border_of(gui.border.as_ref(), border));
    }
    apply_controls(&mut gui, controls, &mut context)?;
    Ok(gui)
}

/// The text of a layout file written from KOTOR's layout `gui`, so that its
/// controls can be copied and changed. It holds names and numbers only: a
/// control's own words are left out, and its talk table line is kept.
pub fn export(gui: &Gui, name: &str) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "# KOTOR's {name}, written out as a standalone layout.\nlayout = \"{name}_copy\"\nframe = [{}, {}]\n",
        gui.frame().0,
        gui.frame().1
    );
    let border = |out: &mut String, key: &str, border: &Border| {
        let _ = writeln!(
            out,
            "{key} = {{ corner = {:?}, edge = {:?}, fill = {:?}, fill_style = {}, dimension = {}, inner_offset = {} }}",
            border.corner,
            border.edge,
            border.fill,
            border.fill_style,
            border.dimension,
            border.inner_offset
        );
    };
    for control in gui.walk() {
        let _ = write!(out, "\n[[control]]\ntag = {:?}\n", control.tag);
        if let Some(kind) = kind_name(control.kind) {
            let _ = writeln!(out, "kind = {kind:?}");
        }
        let Rect {
            left,
            top,
            width,
            height,
        } = control.extent;
        let _ = writeln!(
            out,
            "left = {left}\ntop = {top}\nwidth = {width}\nheight = {height}"
        );
        if let Some(text) = &control.text {
            if !text.font.is_empty() {
                let _ = writeln!(out, "font = {:?}", text.font);
            }
            if let Some(strref) = text.strref {
                let _ = writeln!(out, "strref = {strref}");
            }
        }
        for (key, slot) in BORDERS.into_iter().zip([
            &control.border,
            &control.hilight,
            &control.selected,
            &control.hilight_selected,
        ]) {
            if let Some(slot) = slot {
                border(&mut out, key, slot);
            }
        }
    }
    out
}

/// Which layouts the screens shown are drawn from, in words for the
/// catalogue's status bar: a standalone layout, an override that is applied,
/// an override that is off, or KOTOR's layouts as they are.
pub fn describe<'a>(layouts: impl Iterator<Item = &'a str>, on: bool) -> String {
    let mut said: Vec<String> = Vec::new();
    for name in layouts {
        let line = if let Some(own) = name.strip_prefix(PREFIX) {
            format!("kjk's {own}")
        } else if !override_path(name).is_file() {
            continue;
        } else if on {
            format!("kjk's override of {name}")
        } else {
            format!("KOTOR's {name}, override off")
        };
        if !said.contains(&line) {
            said.push(line);
        }
    }
    if said.is_empty() {
        "KOTOR's".to_string()
    } else {
        said.join(", ")
    }
}

/// Reads a layout file from `path`.
pub fn read(path: &Path, tokens: &Tokens) -> Result<File, String> {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let text = std::fs::read_to_string(path).map_err(|error| format!("{name}: {error}"))?;
    parse(&name, &text, tokens)
}
