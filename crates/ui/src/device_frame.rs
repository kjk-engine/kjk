//! The outline of the device the catalogue's virtual screen stands for
//! (docs/UI.md, "The catalogue in the game"). It is drawn around the image
//! when the window shows the whole device, as a Steam Deck's body with its
//! sticks and buttons, or as a monitor or TV with its bezel and stand. The
//! parts are measured in millimetres from the screen's top left corner, so
//! that the frame has the screen's real proportions at any scale.

use crate::catalogue::Size;
use crate::draw::{DrawList, Quad};
use crate::virtual_screen::{Placement, inches};

/// How a part is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Style {
    /// The device's body: filled around the screen, and outlined.
    Body,
    /// A stand or a trackpad: filled and outlined.
    Solid,
    /// A button or a stick: filled in a lighter grey, and outlined.
    Control,
}

/// A rounded rectangle of `[left, top, width, height]` millimetres. A
/// circle is one whose radius is half its size.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Part {
    rect: [f32; 4],
    radius: f32,
    style: Style,
}

fn round(rect: [f32; 4], radius: f32, style: Style) -> Part {
    Part {
        rect,
        radius,
        style,
    }
}

fn disc([x, y]: [f32; 2], radius: f32, style: Style) -> Part {
    round(
        [x - radius, y - radius, radius * 2.0, radius * 2.0],
        radius,
        style,
    )
}

const BODY: [f32; 4] = [0.13, 0.13, 0.14, 1.0];
const SOLID: [f32; 4] = [0.19, 0.19, 0.2, 1.0];
const CONTROL: [f32; 4] = [0.27, 0.27, 0.28, 1.0];
const LINE: [f32; 4] = [0.5, 0.5, 0.52, 1.0];

/// The screen's width and height in millimetres: the diagonal of the
/// size's typical screen (`virtual_screen::inches`) at the shape of its
/// pixels.
pub fn screen_mm(size: &Size) -> [f32; 2] {
    let diagonal = inches(size) * 25.4;
    let (w, h) = (size.pixels.0 as f32, size.pixels.1 as f32);
    let across = w.hypot(h);
    [diagonal * w / across, diagonal * h / across]
}

/// A monitor or a TV: a body around the screen, and a stand under it.
struct Desk {
    /// The bezel's width at the sides and the top, and under the screen.
    bezel: [f32; 3],
    radius: f32,
    /// The neck's width and height. The base's width and height.
    neck: [f32; 2],
    base: [f32; 2],
}

fn desk([w, h]: [f32; 2], desk: &Desk) -> Vec<Part> {
    let [side, top, chin] = desk.bezel;
    let bottom = h + chin;
    let mut parts = vec![round(
        [-side, -top, w + 2.0 * side, h + top + chin],
        desk.radius,
        Style::Body,
    )];
    if desk.neck[0] > 0.0 {
        parts.push(round(
            [
                w / 2.0 - desk.neck[0] / 2.0,
                bottom,
                desk.neck[0],
                desk.neck[1],
            ],
            0.0,
            Style::Solid,
        ));
    }
    parts.push(round(
        [
            w / 2.0 - desk.base[0] / 2.0,
            bottom + desk.neck[1],
            desk.base[0],
            desk.base[1],
        ],
        desk.base[1] / 2.0,
        Style::Solid,
    ));
    parts
}

/// A TV: a thin bezel, and a foot under each end.
fn tv([w, h]: [f32; 2]) -> Vec<Part> {
    let bottom = h + 12.0;
    let mut parts = vec![round(
        [-7.0, -7.0, w + 14.0, h + 7.0 + 12.0],
        4.0,
        Style::Body,
    )];
    for x in [w * 0.12, w * 0.88 - 200.0] {
        parts.push(round([x, bottom, 200.0, 30.0], 6.0, Style::Solid));
    }
    parts
}

/// A Steam Deck: 298 × 117 mm around a 7 inch screen, with its two sticks,
/// the d-pad, the four buttons and the two trackpads.
fn deck([w, h]: [f32; 2]) -> Vec<Part> {
    let (width, height) = (298.0, 117.0);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut parts = vec![round(
        [cx - width / 2.0, cy - height / 2.0, width, height],
        24.0,
        Style::Body,
    )];
    for side in [-1.0_f32, 1.0] {
        let at = |x: f32, y: f32| [cx + side * x, cy + y];
        let stick = at(100.0, 6.0);
        parts.push(disc(stick, 12.0, Style::Solid));
        parts.push(disc(stick, 8.5, Style::Control));
        let pad = at(92.0, 37.0);
        parts.push(round(
            [pad[0] - 14.0, pad[1] - 13.0, 28.0, 26.0],
            5.0,
            Style::Solid,
        ));
        let cluster = at(116.0, -22.0);
        if side < 0.0 {
            parts.push(round(
                [cluster[0] - 3.5, cluster[1] - 11.0, 7.0, 22.0],
                2.0,
                Style::Control,
            ));
            parts.push(round(
                [cluster[0] - 11.0, cluster[1] - 3.5, 22.0, 7.0],
                2.0,
                Style::Control,
            ));
        } else {
            for (dx, dy) in [(0.0, -10.0), (0.0, 10.0), (-10.0, 0.0), (10.0, 0.0)] {
                parts.push(disc(
                    [cluster[0] + dx, cluster[1] + dy],
                    5.5,
                    Style::Control,
                ));
            }
        }
    }
    parts
}

fn parts_of(size: &Size) -> Vec<Part> {
    let screen = screen_mm(size);
    match size.name {
        "deck" => deck(screen),
        "4k" => tv(screen),
        "4x3" => desk(
            screen,
            &Desk {
                bezel: [25.0, 25.0, 40.0],
                radius: 4.0,
                neck: [50.0, 80.0],
                base: [220.0, 14.0],
            },
        ),
        "ultrawide" => desk(
            screen,
            &Desk {
                bezel: [7.0, 7.0, 17.0],
                radius: 3.0,
                neck: [60.0, 100.0],
                base: [300.0, 12.0],
            },
        ),
        _ => desk(
            screen,
            &Desk {
                bezel: [8.0, 8.0, 18.0],
                radius: 3.0,
                neck: [40.0, 90.0],
                base: [240.0, 12.0],
            },
        ),
    }
}

/// How far the device reaches past its screen on the left, top, right and
/// bottom, in the device's pixels.
pub fn margin(size: &Size) -> [f32; 4] {
    let screen = screen_mm(size);
    let per_mm = size.pixels.0 as f32 / screen[0];
    let mut edges = [0.0_f32; 4];
    for part in parts_of(size) {
        let [x, y, w, h] = part.rect;
        edges[0] = edges[0].max(-x);
        edges[1] = edges[1].max(-y);
        edges[2] = edges[2].max(x + w - screen[0]);
        edges[3] = edges[3].max(y + h - screen[1]);
    }
    edges.map(|edge| edge * per_mm)
}

/// The device drawn around the image where `placed` puts it, in a window of
/// `window` logical pixels. Its lines are `line` logical pixels thick.
pub fn draw(size: &Size, placed: &Placement, window: (f32, f32), line: f32) -> DrawList {
    let screen = screen_mm(size);
    let k = size.pixels.0 as f32 / screen[0] * placed.scale;
    let hole = [
        placed.origin[0],
        placed.origin[1],
        placed.size[0],
        placed.size[1],
    ];
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    for part in parts_of(size) {
        let [x, y, w, h] = part.rect;
        let rect = [
            placed.origin[0] + x * k,
            placed.origin[1] + y * k,
            w * k,
            h * k,
        ];
        let (fill, hole) = match part.style {
            Style::Body => (BODY, Some(hole)),
            Style::Solid => (SOLID, None),
            Style::Control => (CONTROL, None),
        };
        fill_round(&mut out, rect, part.radius * k, hole, fill);
        outline_round(&mut out, rect, part.radius * k, line, LINE);
        if let Some([x, y, w, h]) = hole {
            let grown = [x - line, y - line, w + 2.0 * line, h + 2.0 * line];
            outline_round(&mut out, grown, 0.0, line, LINE);
        }
    }
    out
}

/// How many strips or segments a corner of `radius` pixels is drawn with.
fn steps(radius: f32) -> usize {
    ((radius / 3.0) as usize).clamp(3, 24)
}

/// A rounded rectangle filled with horizontal strips, with `hole` left out.
fn fill_round(
    out: &mut DrawList,
    [x, y, w, h]: [f32; 4],
    radius: f32,
    hole: Option<[f32; 4]>,
    color: [f32; 4],
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let radius = radius.min(w / 2.0).min(h / 2.0);
    let n = steps(radius);
    let mut edges = vec![y, y + h];
    if radius > 0.0 {
        for i in 1..n {
            let d = radius * i as f32 / n as f32;
            edges.push(y + d);
            edges.push(y + h - d);
        }
        edges.push(y + radius);
        edges.push(y + h - radius);
    }
    if let Some([_, hy, _, hh]) = hole {
        edges.push(hy.clamp(y, y + h));
        edges.push((hy + hh).clamp(y, y + h));
    }
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    for pair in edges.windows(2) {
        let (top, bottom) = (pair[0], pair[1]);
        let middle = (top + bottom) / 2.0;
        let from_edge = (middle - y).min(y + h - middle);
        let inset = if from_edge < radius {
            radius
                - (radius * radius - (radius - from_edge).powi(2))
                    .max(0.0)
                    .sqrt()
        } else {
            0.0
        };
        let (left, right) = (x + inset, x + w - inset);
        let mut spans = vec![(left, right)];
        if let Some([hx, hy, hw, hh]) = hole
            && middle > hy
            && middle < hy + hh
        {
            spans = vec![(left, hx.min(right)), (hx + hw, right)];
        }
        for (a, b) in spans {
            if b > a {
                out.quads
                    .push(Quad::solid([a, top, b - a, bottom - top], color));
            }
        }
    }
}

/// A line of `thick` pixels from `a` to `b`, as a turned quad.
fn segment(out: &mut DrawList, a: [f32; 2], b: [f32; 2], thick: f32, color: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy) + thick;
    let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let mut quad = Quad::solid(
        [
            middle[0] - length / 2.0,
            middle[1] - thick / 2.0,
            length,
            thick,
        ],
        color,
    );
    quad.spin = dy.atan2(dx);
    out.quads.push(quad);
}

/// The outline of a rounded rectangle, a circle when its radius is half its
/// size.
fn outline_round(
    out: &mut DrawList,
    [x, y, w, h]: [f32; 4],
    radius: f32,
    thick: f32,
    color: [f32; 4],
) {
    let radius = radius.min(w / 2.0).min(h / 2.0);
    if radius <= 0.0 {
        for rect in [
            [x, y, w, thick],
            [x, y + h - thick, w, thick],
            [x, y, thick, h],
            [x + w - thick, y, thick, h],
        ] {
            out.quads.push(Quad::solid(rect, color));
        }
        return;
    }
    let n = steps(radius) * 2;
    let corners = [
        ([x + w - radius, y + radius], -90.0_f32),
        ([x + w - radius, y + h - radius], 0.0),
        ([x + radius, y + h - radius], 90.0),
        ([x + radius, y + radius], 180.0),
    ];
    let mut points: Vec<[f32; 2]> = Vec::new();
    for (centre, from) in corners {
        for i in 0..=n / 4 {
            let angle = (from + 90.0 * i as f32 / (n / 4) as f32).to_radians();
            points.push([
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]);
        }
    }
    for i in 0..points.len() {
        segment(out, points[i], points[(i + 1) % points.len()], thick, color);
    }
}

/// The device a size stands for, in words: "Steam Deck, 1280 × 800".
pub fn describe(size: &Size) -> String {
    let device = match size.name {
        "deck" => "Steam Deck",
        "4k" => "4K",
        "4x3" => "4:3 monitor",
        "ultrawide" => "Ultrawide monitor",
        _ => "1080p monitor",
    };
    format!("{device}, {} × {}", size.pixels.0, size.pixels.1)
}
