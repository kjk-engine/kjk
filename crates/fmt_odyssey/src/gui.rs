//! GUI: a screen's layout, a GFF (`GUI `) whose root is a panel with a
//! list of controls (`CONTROLS`), each a struct that may hold controls of
//! its own. Positions are pixels of the screen the layout was made for
//! (the root's extent, 640 × 480 for most), from the top left.

use crate::FormatError;
use crate::gff::{Gff, GffStruct, GffValue};

const FORMAT: &str = "GUI";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// `CONTROLTYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Control,
    Panel,
    Label,
    /// A list box's row with an icon at its left (an image button).
    ProtoItem,
    Button,
    CheckBox,
    Slider,
    ScrollBar,
    Progress,
    ListBox,
    Other(i32),
}

impl Kind {
    pub fn of(number: i32) -> Kind {
        match number {
            0 => Kind::Control,
            2 => Kind::Panel,
            4 => Kind::Label,
            5 => Kind::ProtoItem,
            6 => Kind::Button,
            7 => Kind::CheckBox,
            8 => Kind::Slider,
            9 => Kind::ScrollBar,
            10 => Kind::Progress,
            11 => Kind::ListBox,
            other => Kind::Other(other),
        }
    }
}

/// A frame drawn from three textures: one corner (turned for each of the
/// four), one edge (turned for each side) and a fill.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Border {
    pub corner: String,
    pub edge: String,
    pub fill: String,
    pub fill_style: i32,
    /// The corner's and edge's thickness in pixels.
    pub dimension: i32,
    pub inner_offset: i32,
    pub inner_offset_y: Option<i32>,
    /// `None` where the layout leaves it unset (or writes -1).
    pub color: Option<[f32; 3]>,
    pub pulsing: bool,
}

/// `Text::alignment`'s bits: one across (`ACROSS`), one down (`DOWN`).
pub mod align {
    pub const LEFT: i32 = 1;
    pub const CENTRE: i32 = 2;
    pub const RIGHT: i32 = 4;
    pub const TOP: i32 = 8;
    pub const MIDDLE: i32 = 16;
    pub const BOTTOM: i32 = 32;
    pub const ACROSS: i32 = LEFT | CENTRE | RIGHT;
    pub const DOWN: i32 = TOP | MIDDLE | BOTTOM;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Text {
    pub font: String,
    /// Written in the layout, in the game's code page; used when `strref`
    /// is none.
    pub text: Vec<u8>,
    pub strref: Option<u32>,
    pub alignment: i32,
    /// `None` where the layout leaves it unset (or writes -1).
    pub color: Option<[f32; 3]>,
    pub pulsing: bool,
}

/// The controls (by `ID`) a direction moves to; -1 for none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveTo {
    pub up: i32,
    pub down: i32,
    pub left: i32,
    pub right: i32,
}

/// A picture inside a control: a scroll bar's thumb or arrow, a slider's
/// thumb.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Image {
    pub image: String,
    pub draw_style: i32,
    pub flip_style: i32,
    pub rotate: f32,
    pub alignment: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Control {
    pub kind: Kind,
    pub id: i32,
    pub tag: String,
    /// `Obj_ParentID`: -1 for the root panel.
    pub parent: i32,
    pub extent: Rect,
    pub border: Option<Border>,
    pub hilight: Option<Border>,
    pub selected: Option<Border>,
    pub hilight_selected: Option<Border>,
    pub text: Option<Text>,
    pub moveto: Option<MoveTo>,
    /// `None` where the layout writes -1 (keep the default).
    pub color: Option<[f32; 3]>,
    pub alpha: Option<f32>,
    pub padding: i32,
    pub looping: bool,
    pub left_scrollbar: bool,
    /// A list box's row, repeated for each item.
    pub proto: Option<Box<Control>>,
    pub scrollbar: Option<Box<Control>>,
    pub thumb: Option<Image>,
    pub dir: Option<Image>,
    /// A progress bar's fill.
    pub progress: Option<Border>,
    pub start_from_left: Option<bool>,
    /// (current, maximum): a progress bar's or scroll bar's values.
    pub value: Option<(i32, i32)>,
    pub children: Vec<Control>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gui {
    pub tag: String,
    pub extent: Rect,
    pub border: Option<Border>,
    pub color: Option<[f32; 3]>,
    pub alpha: Option<f32>,
    pub controls: Vec<Control>,
    /// `TAG.LABEL` of every field present and not read, for the sweep.
    pub unread: Vec<String>,
}

fn unsupported(what: String) -> FormatError {
    FormatError::Unsupported {
        format: FORMAT,
        what,
    }
}

/// What a struct's fields were read for, so the rest can be listed.
struct Fields<'a> {
    record: &'a GffStruct,
    read: Vec<&'static str>,
}

impl<'a> Fields<'a> {
    fn new(record: &'a GffStruct) -> Fields<'a> {
        Fields {
            record,
            read: Vec::new(),
        }
    }

    fn get(&mut self, label: &'static str) -> Option<&'a GffValue> {
        self.read.push(label);
        self.record.get(label)
    }

    fn int(&mut self, label: &'static str) -> Option<i32> {
        self.get(label)?.int().map(|value| value as i32)
    }

    fn flag(&mut self, label: &'static str) -> bool {
        self.int(label).is_some_and(|value| value != 0)
    }

    fn string(&mut self, label: &'static str) -> Option<Vec<u8>> {
        self.get(label)?.bytes().map(<[u8]>::to_vec)
    }

    fn name(&mut self, label: &'static str) -> String {
        self.get(label)
            .and_then(GffValue::resref)
            .unwrap_or_else(String::new)
    }

    fn float(&mut self, label: &'static str) -> Option<f32> {
        self.get(label)?.float()
    }

    fn color(&mut self, label: &'static str) -> Option<[f32; 3]> {
        match self.get(label)? {
            GffValue::Vector(bits) => {
                let color = bits.map(f32::from_bits);
                (color != [-1.0; 3]).then_some(color)
            }
            _ => None,
        }
    }

    fn record(&mut self, label: &'static str) -> Option<&'a GffStruct> {
        match self.get(label)? {
            GffValue::Struct(record) => Some(record),
            _ => None,
        }
    }

    fn unread(&self, tag: &str, out: &mut Vec<String>) {
        for field in &self.record.fields {
            let label = String::from_utf8_lossy(&field.label);
            if !self.read.iter().any(|read| *read == label) {
                out.push(format!("{tag}.{label}"));
            }
        }
    }
}

fn extent(record: &GffStruct) -> Option<Rect> {
    let mut fields = Fields::new(record);
    Some(Rect {
        left: fields.int("LEFT")?,
        top: fields.int("TOP")?,
        width: fields.int("WIDTH")?,
        height: fields.int("HEIGHT")?,
    })
}

fn border(record: &GffStruct) -> Border {
    let mut fields = Fields::new(record);
    Border {
        corner: fields.name("CORNER"),
        edge: fields.name("EDGE"),
        fill: fields.name("FILL"),
        fill_style: fields.int("FILLSTYLE").unwrap_or(0),
        dimension: fields.int("DIMENSION").unwrap_or(0),
        inner_offset: fields.int("INNEROFFSET").unwrap_or(0),
        inner_offset_y: fields.int("INNEROFFSETY"),
        color: fields.color("COLOR"),
        pulsing: fields.flag("PULSING"),
    }
}

fn text(record: &GffStruct) -> Text {
    let mut fields = Fields::new(record);
    Text {
        font: fields.name("FONT"),
        text: fields.string("TEXT").unwrap_or(Vec::new()),
        strref: fields
            .get("STRREF")
            .and_then(|value| match value {
                GffValue::StrRef(n) => Some(*n),
                other => other.uint(),
            })
            .filter(|strref| *strref != u32::MAX),
        alignment: fields.int("ALIGNMENT").unwrap_or(0),
        color: fields.color("COLOR"),
        pulsing: fields.flag("PULSING"),
    }
}

fn image(record: &GffStruct) -> Image {
    let mut fields = Fields::new(record);
    Image {
        image: fields.name("IMAGE"),
        draw_style: fields.int("DRAWSTYLE").unwrap_or(0),
        flip_style: fields.int("FLIPSTYLE").unwrap_or(0),
        rotate: fields.float("ROTATE").unwrap_or(0.0),
        alignment: fields.int("ALIGNMENT").unwrap_or(0),
    }
}

fn control(record: &GffStruct, unread: &mut Vec<String>) -> Result<Control, FormatError> {
    let mut fields = Fields::new(record);
    let tag = String::from_utf8_lossy(&fields.string("TAG").unwrap_or(Vec::new())).into_owned();
    let extent = fields
        .record("EXTENT")
        .and_then(extent)
        .ok_or_else(|| unsupported(format!("control {tag:?} without an extent")))?;
    let moveto = fields.record("MOVETO").map(|record| {
        let mut links = Fields::new(record);
        MoveTo {
            up: links.int("UP").unwrap_or(-1),
            down: links.int("DOWN").unwrap_or(-1),
            left: links.int("LEFT").unwrap_or(-1),
            right: links.int("RIGHT").unwrap_or(-1),
        }
    });
    let mut nested = |label: &'static str, fields: &mut Fields| -> Result<_, FormatError> {
        fields
            .record(label)
            .map(|record| control(record, unread).map(Box::new))
            .transpose()
    };
    let proto = nested("PROTOITEM", &mut fields)?;
    let scrollbar = nested("SCROLLBAR", &mut fields)?;
    let children = match fields.get("CONTROLS") {
        Some(GffValue::List(list)) => list
            .iter()
            .map(|record| control(record, unread))
            .collect::<Result<_, _>>()?,
        _ => Vec::new(),
    };
    let current = fields.int("CURVALUE");
    let maximum = fields.int("MAXVALUE");
    let parsed = Control {
        kind: Kind::of(fields.int("CONTROLTYPE").unwrap_or(0)),
        id: fields.int("ID").unwrap_or(-1),
        parent: fields.int("Obj_ParentID").unwrap_or(-1),
        extent,
        border: fields.record("BORDER").map(border),
        hilight: fields.record("HILIGHT").map(border),
        selected: fields.record("SELECTED").map(border),
        hilight_selected: fields.record("HILIGHTSELECTED").map(border),
        text: fields.record("TEXT").map(text),
        moveto,
        color: fields.color("COLOR"),
        alpha: fields.float("ALPHA"),
        padding: fields.int("PADDING").unwrap_or(0),
        looping: fields.flag("LOOPING"),
        left_scrollbar: fields.flag("LEFTSCROLLBAR"),
        proto,
        scrollbar,
        thumb: fields.record("THUMB").map(image),
        dir: fields.record("DIR").map(image),
        progress: fields.record("PROGRESS").map(border),
        start_from_left: fields.int("STARTFROMLEFT").map(|n| n != 0),
        value: match (current, maximum) {
            (Some(current), Some(maximum)) => Some((current, maximum)),
            (Some(current), None) => Some((current, 100)),
            _ => None,
        },
        children,
        tag,
    };
    for label in ["Obj_Locked", "Obj_Parent", "DRAWMODE", "VISIBLEVALUE"] {
        fields.get(label);
    }
    fields.unread(&parsed.tag, unread);
    Ok(parsed)
}

impl Gui {
    pub fn parse(bytes: &[u8]) -> Result<Gui, FormatError> {
        Gui::from_gff(&Gff::parse(bytes)?)
    }

    pub fn from_gff(gff: &Gff) -> Result<Gui, FormatError> {
        let mut fields = Fields::new(&gff.root);
        let mut unread = Vec::new();
        let tag = String::from_utf8_lossy(&fields.string("TAG").unwrap_or(Vec::new())).into_owned();
        let extent = fields
            .record("EXTENT")
            .and_then(extent)
            .ok_or_else(|| unsupported(format!("layout {tag:?} without an extent")))?;
        let controls = match fields.get("CONTROLS") {
            Some(GffValue::List(list)) => list
                .iter()
                .map(|record| control(record, &mut unread))
                .collect::<Result<_, _>>()?,
            _ => Vec::new(),
        };
        let gui = Gui {
            border: fields.record("BORDER").map(border),
            color: fields.color("COLOR"),
            alpha: fields.float("ALPHA"),
            tag,
            extent,
            controls,
            unread: Vec::new(),
        };
        for label in [
            "CONTROLTYPE",
            "ID",
            "Obj_Locked",
            "Obj_Parent",
            "Obj_ParentID",
        ] {
            fields.get(label);
        }
        fields.unread(&gui.tag, &mut unread);
        Ok(Gui { unread, ..gui })
    }

    /// The screen the layout was made for: 640 × 480 unless the root
    /// reaches past it. Controls are placed from the root's top left.
    pub fn frame(&self) -> (i32, i32) {
        let right = self.extent.left + self.extent.width;
        let bottom = self.extent.top + self.extent.height;
        if right <= 640 && bottom <= 480 {
            (640, 480)
        } else {
            (right, bottom)
        }
    }

    /// Every control, depth first, children after their parent.
    pub fn walk(&self) -> impl Iterator<Item = &Control> {
        let mut stack: Vec<&Control> = self.controls.iter().rev().collect();
        std::iter::from_fn(move || {
            let next = stack.pop()?;
            stack.extend(next.children.iter().rev());
            Some(next)
        })
    }

    pub fn find(&self, tag: &str) -> Option<&Control> {
        self.walk()
            .find(|control| control.tag.eq_ignore_ascii_case(tag))
    }

    pub fn by_id(&self, id: i32) -> Option<&Control> {
        self.walk().find(|control| control.id == id)
    }
}
