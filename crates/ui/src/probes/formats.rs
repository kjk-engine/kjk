//! Layouts and fonts parsed from data written here.

use asset_odyssey::font::{Font, FontError};
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};
use fmt_odyssey::gui::{Gui, Kind};
use fmt_odyssey::txi::Txi;

use super::{Check, check, expect, near};

pub(super) fn field(label: &str, value: GffValue) -> GffField {
    GffField {
        label: label.as_bytes().to_vec(),
        value,
    }
}

pub(super) fn record(fields: Vec<GffField>) -> GffStruct {
    GffStruct { id: 0, fields }
}

pub(super) fn int(label: &str, value: i32) -> GffField {
    field(label, GffValue::Int(value))
}

pub(super) fn text(label: &str, value: &str) -> GffField {
    field(label, GffValue::String(value.as_bytes().to_vec()))
}

pub(super) fn resref(label: &str, value: &str) -> GffField {
    field(label, GffValue::ResRef(value.as_bytes().to_vec()))
}

pub(super) fn vector(label: &str, value: [f32; 3]) -> GffField {
    field(label, GffValue::Vector(value.map(f32::to_bits)))
}

pub(super) fn extent(left: i32, top: i32, width: i32, height: i32) -> GffField {
    field(
        "EXTENT",
        GffValue::Struct(Box::new(record(vec![
            int("LEFT", left),
            int("TOP", top),
            int("WIDTH", width),
            int("HEIGHT", height),
        ]))),
    )
}

pub(super) fn border(label: &str, corner: &str, dimension: i32) -> GffField {
    field(
        label,
        GffValue::Struct(Box::new(record(vec![
            resref("CORNER", corner),
            resref("EDGE", "Edge"),
            resref("FILL", ""),
            int("FILLSTYLE", 2),
            int("DIMENSION", dimension),
            int("INNEROFFSET", 0),
            vector("COLOR", [1.0, 1.0, 1.0]),
            field("PULSING", GffValue::Byte(0)),
        ]))),
    )
}

pub(super) fn caption(strref: u32, alignment: i32, pulsing: u8) -> GffField {
    field(
        "TEXT",
        GffValue::Struct(Box::new(record(vec![
            int("ALIGNMENT", alignment),
            vector("COLOR", [0.0, 0.5, 1.0]),
            resref("FONT", "DialogFont16x16"),
            text("TEXT", ""),
            field("STRREF", GffValue::Dword(strref)),
            field("PULSING", GffValue::Byte(pulsing)),
        ]))),
    )
}

pub(super) fn moveto(up: i32, down: i32, left: i32, right: i32) -> GffField {
    field(
        "MOVETO",
        GffValue::Struct(Box::new(record(vec![
            int("UP", up),
            int("DOWN", down),
            int("LEFT", left),
            int("RIGHT", right),
        ]))),
    )
}

pub(super) fn control(kind: i32, id: i32, tag: &str, mut more: Vec<GffField>) -> GffStruct {
    let mut fields = vec![
        int("CONTROLTYPE", kind),
        int("ID", id),
        text("TAG", tag),
        int("Obj_ParentID", -1),
        extent(10, 20, 100, 30),
    ];
    fields.append(&mut more);
    record(fields)
}

pub(super) fn layout(controls: Vec<GffStruct>, root_extent: bool) -> Gff {
    let mut fields = vec![int("CONTROLTYPE", 2), text("TAG", "TGuiPanel")];
    if root_extent {
        fields.push(extent(0, 0, 640, 480));
    }
    fields.push(field("CONTROLS", GffValue::List(controls)));
    Gff {
        file_type: *b"GUI ",
        root: record(fields),
    }
}

pub(super) fn layouts() -> Vec<Check> {
    let button = control(
        6,
        3,
        "BTN_A",
        vec![
            border("BORDER", "Corner", 4),
            border("HILIGHT", "Lit", 6),
            caption(42, 18, 1),
            moveto(-1, 4, -1, -1),
            vector("COLOR", [-1.0, -1.0, -1.0]),
            int("FOO", 7),
        ],
    );
    let list = control(
        11,
        4,
        "LB_ITEMS",
        vec![
            border("BORDER", "", 4),
            field(
                "PROTOITEM",
                GffValue::Struct(Box::new(control(
                    6,
                    -1,
                    "PROTOITEM",
                    vec![caption(u32::MAX, 18, 0)],
                ))),
            ),
            field(
                "SCROLLBAR",
                GffValue::Struct(Box::new(control(9, -1, "SCROLLBAR", vec![]))),
            ),
            field("LEFTSCROLLBAR", GffValue::Byte(1)),
            int("PADDING", 5),
        ],
    );
    let odd = control(42, 5, "ODD", vec![]);
    let parsed = Gui::from_gff(&layout(vec![button, list, odd], true));
    vec![
        check(
            "gui_tree",
            parsed
                .as_ref()
                .map_err(ToString::to_string)
                .and_then(|gui| {
                    let button = gui.find("BTN_A").ok_or("no BTN_A")?;
                    expect("the root's tag", gui.tag == "TGuiPanel")?;
                    expect("the root's extent", gui.extent.width == 640)?;
                    expect("a button", button.kind == Kind::Button)?;
                    expect("its id", button.id == 3)?;
                    expect(
                        "its extent",
                        button.extent.left == 10 && button.extent.height == 30,
                    )?;
                    let edge = button.border.as_ref().ok_or("no border")?;
                    expect(
                        "resrefs lowercased",
                        edge.corner == "corner" && edge.edge == "edge",
                    )?;
                    expect(
                        "the border's size",
                        edge.dimension == 4 && edge.fill_style == 2,
                    )?;
                    let lit = button.hilight.as_ref().ok_or("no hilight")?;
                    expect("the hilight", lit.corner == "lit" && lit.dimension == 6)?;
                    let words = button.text.as_ref().ok_or("no text")?;
                    expect("the strref", words.strref == Some(42))?;
                    expect("the font", words.font == "dialogfont16x16")?;
                    expect(
                        "alignment and pulse",
                        words.alignment == 18 && words.pulsing,
                    )?;
                    let links = button.moveto.ok_or("no MOVETO")?;
                    expect("MOVETO", links.down == 4 && links.up == -1)?;
                    expect("COLOR -1 is none", button.color.is_none())?;
                    Ok(format!("{} controls", gui.walk().count()))
                }),
        ),
        check(
            "gui_listbox",
            parsed
                .as_ref()
                .map_err(ToString::to_string)
                .and_then(|gui| {
                    let list = gui.find("LB_ITEMS").ok_or("no LB_ITEMS")?;
                    expect("a list box", list.kind == Kind::ListBox)?;
                    let proto = list.proto.as_ref().ok_or("no PROTOITEM")?;
                    expect("its prototype", proto.kind == Kind::Button)?;
                    let words = proto.text.as_ref().ok_or("no prototype text")?;
                    expect("no strref", words.strref.is_none())?;
                    let bar = list.scrollbar.as_ref().ok_or("no SCROLLBAR")?;
                    expect("its scroll bar", bar.kind == Kind::ScrollBar)?;
                    expect("left and padded", list.left_scrollbar && list.padding == 5)?;
                    Ok("prototype, scroll bar".into())
                }),
        ),
        check(
            "gui_unknown_kind",
            parsed
                .as_ref()
                .map_err(ToString::to_string)
                .and_then(|gui| {
                    let odd = gui.find("ODD").ok_or("no ODD")?;
                    expect("kept as Other", odd.kind == Kind::Other(42))?;
                    Ok("Other(42)".into())
                }),
        ),
        check(
            "gui_unread",
            parsed
                .as_ref()
                .map_err(ToString::to_string)
                .and_then(|gui| {
                    expect(
                        &format!("FOO listed: {:?}", gui.unread),
                        gui.unread.iter().any(|label| label == "BTN_A.FOO"),
                    )?;
                    Ok(format!("{} unread", gui.unread.len()))
                }),
        ),
        check(
            "gui_missing_extent",
            match Gui::from_gff(&layout(vec![], false)) {
                Ok(_) => Err("a layout without an extent parsed".into()),
                Err(error) => Ok(error.to_string()),
            },
        ),
    ]
}

/// Four glyphs in a 2 × 2 grid on a 32 × 32 sheet, 16 px tall, with a
/// pixel of spacing after each and two between lines.
pub(super) const GRID: &str = "numchars 4
fontheight 0.16
spacingR 0.01
spacingB 0.02
upperleftcoords 4
0 1 0
0.5 1 0
0 0.5 0
0.5 0.5 0
lowerrightcoords 4
0.5 0.5 0
1 0.5 0
0.5 0 0
1 0 0
";

pub(super) fn fonts() -> Vec<Check> {
    let grid = Font::from_txi("grid", &Txi::parse(GRID.as_bytes()), 32, 32);
    vec![
        check(
            "font_glyphs",
            grid.as_ref()
                .map_err(|e| format!("{e:?}"))
                .and_then(|font| {
                    expect("four glyphs", font.glyphs.len() == 4)?;
                    let last = &font.glyphs[3];
                    expect(
                        &format!("glyph 3's uv, upright: {:?}", last.uv),
                        last.uv == [0.5, 0.5, 1.0, 1.0],
                    )?;
                    expect(
                        "16 px square",
                        near(last.width, 16.0) && near(last.height, 16.0),
                    )?;
                    expect("line pitch 18", near(font.line_height(), 18.0))?;
                    Ok("2 × 2 grid".into())
                }),
        ),
        check(
            "font_measure",
            grid.as_ref()
                .map_err(|e| format!("{e:?}"))
                .and_then(|font| {
                    let two = font.measure(&[0, 1]);
                    expect(
                        &format!("two glyphs measure 34, not {two}"),
                        near(two, 34.0),
                    )?;
                    let missing = font.advance(9);
                    expect(
                        &format!("a byte past the table advances by spacingR, not {missing}"),
                        near(missing, 1.0),
                    )?;
                    Ok(format!("{two} px"))
                }),
        ),
        check(
            "font_without_table",
            match Font::from_txi("plain", &Txi::parse(b"fontheight 0.16"), 32, 32) {
                Err(FontError::NoGlyphTable) => Ok("refused".into()),
                other => Err(format!("{other:?}")),
            },
        ),
    ]
}
