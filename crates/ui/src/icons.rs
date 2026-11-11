//! A pad's buttons drawn as the screens show them (the key mapping's
//! controller tab): no game's art and no maker's, shapes of our own made
//! here. A texture named `kjkpad_<kind>_<chord>` (`kjkpad_xbox_lb+south`,
//! any case, as `Textures::texture` lowercases it) is the chord's buttons
//! side by side in that kind's look: Xbox's coloured A B X Y, PlayStation's
//! shapes, Nintendo's and the Steam Deck's lettered greys, each one's
//! names for the shoulders, sticks and middle buttons.

/// The prefix of the textures made here.
pub const PREFIX: &str = "kjkpad_";

/// A button's height (pixels); the chord's `+` and the gap between.
const SIDE: u32 = 48;
const PLUS: u32 = 22;

type Rgba = [f32; 4];

const DARK: Rgba = [0.16, 0.18, 0.21, 1.0];
const EDGE: Rgba = [0.62, 0.66, 0.71, 1.0];
const INK: Rgba = [0.93, 0.94, 0.95, 1.0];

/// The texture `name` names (its lowercased name), if it is one made here:
/// its width, height and RGBA bytes.
pub fn pad_icon(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let (kind, chord) = name.strip_prefix(PREFIX)?.split_once('_')?;
    let kind = Kind::named(kind)?;
    let (shifted, button) = match chord.strip_prefix("lb+") {
        Some(button) => (true, button),
        None => (false, chord),
    };
    let button = Button::named(button)?;
    let mut parts = Vec::new();
    if shifted {
        parts.push(Part::Button(Button::LeftBumper));
        parts.push(Part::Plus);
    }
    parts.push(Part::Button(button));
    let width: u32 = parts.iter().map(|part| part.width()).sum();
    let mut canvas = Canvas::new(width, SIDE);
    let mut left = 0.0;
    for part in &parts {
        match part {
            Part::Button(button) => kind.draw(*button, &mut canvas, left),
            Part::Plus => canvas.text(
                "+",
                [left + PLUS as f32 / 2.0, SIDE as f32 / 2.0],
                14.0,
                INK,
            ),
        }
        left += part.width() as f32;
    }
    Some((width, SIDE, canvas.bytes()))
}

/// The equip page's headgear eye (`pages::Equip`): open while what is worn
/// on the head shows, struck through while it is hidden, on a pad
/// button's dark tile.
const EYE_PREFIX: &str = "kjkeye_";
const EYE_SIDE: u32 = 48;

/// The eye's texture: open (`shown`) or struck through.
pub fn eye_texture(shown: bool) -> String {
    format!("{EYE_PREFIX}{}", if shown { "shown" } else { "hidden" })
}

/// The eye texture `name` names (`eye_texture`), if it is one: its width,
/// height and RGBA bytes.
pub fn eye_icon(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let shown = match name.strip_prefix(EYE_PREFIX)? {
        "shown" => true,
        "hidden" => false,
        _ => return None,
    };
    let side = EYE_SIDE as f32;
    let middle = [side / 2.0, side / 2.0];
    let mut canvas = Canvas::new(EYE_SIDE, EYE_SIDE);
    canvas.round_rect(middle, [side / 2.0 - 1.0; 2], 9.0, DARK, EDGE);
    // The lids: where two discs above and below the middle overlap, an
    // almond 32 wide and 18 tall.
    let (half_wide, half_tall) = (16.0f32, 9.0f32);
    let radius = (half_wide * half_wide + half_tall * half_tall) / (2.0 * half_tall);
    let lids = move |p: [f32; 2]| {
        let up = length(p[0] - middle[0], p[1] - (middle[1] - half_tall + radius)) - radius;
        let down = length(p[0] - middle[0], p[1] - (middle[1] + half_tall - radius)) - radius;
        up.max(down)
    };
    canvas.fill(INK, move |p| lids(p).abs() - 1.5);
    canvas.disc(middle, 5.5, INK);
    if !shown {
        let (a, b) = ([12.0, 36.0], [36.0, 12.0]);
        canvas.line(a, b, 7.0, DARK);
        canvas.line(a, b, 3.0, INK);
    }
    Some((EYE_SIDE, EYE_SIDE, canvas.bytes()))
}

/// A keyboard key's empty cap, its key's name written over it by the
/// screen in the game's own font. `kjkkey_<n>` is a cap wide enough for a
/// name of n letters (1 for `J`, 2 for `F5`).
const KEY_PREFIX: &str = "kjkkey_";
/// How much wider a cap grows for each letter past the first (pixels).
const KEY_LETTER: u32 = 18;

/// The cap's texture for a key name of `letters` letters.
pub fn key_texture(letters: usize) -> String {
    format!("{KEY_PREFIX}{}", letters.clamp(1, 8))
}

/// The cap texture `name` names (`key_texture`), if it is one: its width,
/// height and RGBA bytes.
pub fn key_icon(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let letters: u32 = name.strip_prefix(KEY_PREFIX)?.parse().ok()?;
    let width = SIDE + KEY_LETTER * letters.clamp(1, 8).saturating_sub(1);
    let middle = [width as f32 / 2.0, SIDE as f32 / 2.0];
    let half = [width as f32 / 2.0 - 2.0, SIDE as f32 / 2.0 - 2.0];
    let mut canvas = Canvas::new(width, SIDE);
    canvas.round_rect(middle, half, 9.0, DARK, EDGE);
    Some((width, SIDE, canvas.bytes()))
}

/// The texture of a chord (`LB+South`) on `kind`'s pad (`xbox`).
pub fn pad_texture(kind: &str, chord: &str) -> String {
    format!("{PREFIX}{kind}_{}", chord.to_ascii_lowercase())
}

enum Part {
    Button(Button),
    Plus,
}

impl Part {
    fn width(&self) -> u32 {
        match self {
            Part::Button(button) => button.width(),
            Part::Plus => PLUS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    LeftStick,
    RightStick,
    Up,
    Down,
    Left,
    Right,
    Select,
    Start,
    Mode,
}

impl Button {
    /// By its name in the settings (`gamepad::GamepadButton`'s), lowercased.
    fn named(name: &str) -> Option<Button> {
        use Button::*;
        Some(match name {
            "south" => South,
            "east" => East,
            "west" => West,
            "north" => North,
            "lefttrigger" => LeftBumper,
            "righttrigger" => RightBumper,
            "lefttrigger2" => LeftTrigger,
            "righttrigger2" => RightTrigger,
            "leftthumb" => LeftStick,
            "rightthumb" => RightStick,
            "dpadup" => Up,
            "dpaddown" => Down,
            "dpadleft" => Left,
            "dpadright" => Right,
            "select" => Select,
            "start" => Start,
            "mode" => Mode,
            _ => return None,
        })
    }

    /// Shoulders and triggers are wide; the rest square.
    fn width(self) -> u32 {
        match self {
            Button::LeftBumper
            | Button::RightBumper
            | Button::LeftTrigger
            | Button::RightTrigger => SIDE * 3 / 2,
            _ => SIDE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Xbox,
    PlayStation,
    Nintendo,
    Deck,
}

/// The face buttons' colours on an Xbox pad (south, east, west, north).
const XBOX_FACE: [Rgba; 4] = [
    [0.25, 0.68, 0.29, 1.0],
    [0.88, 0.27, 0.23, 1.0],
    [0.18, 0.47, 0.88, 1.0],
    [0.95, 0.71, 0.11, 1.0],
];

/// PlayStation's shapes' colours (cross, circle, square, triangle).
const PS_FACE: [Rgba; 4] = [
    [0.48, 0.65, 1.0, 1.0],
    [1.0, 0.42, 0.42, 1.0],
    [0.94, 0.54, 0.84, 1.0],
    [0.25, 0.82, 0.63, 1.0],
];

impl Kind {
    fn named(name: &str) -> Option<Kind> {
        Some(match name {
            "xbox" => Kind::Xbox,
            "playstation" => Kind::PlayStation,
            "nintendo" => Kind::Nintendo,
            "deck" => Kind::Deck,
            _ => return None,
        })
    }

    /// The words on a shoulder, a trigger, a stick (`LB`, `L2`, `ZL`, `LS`).
    fn label(self, button: Button) -> &'static str {
        use Button::*;
        use Kind::*;
        match (self, button) {
            (Xbox, LeftBumper) => "LB",
            (Xbox, RightBumper) => "RB",
            (Xbox, LeftTrigger) => "LT",
            (Xbox, RightTrigger) => "RT",
            (Xbox | Nintendo, LeftStick) => "LS",
            (Xbox | Nintendo, RightStick) => "RS",
            (Nintendo, LeftBumper) => "L",
            (Nintendo, RightBumper) => "R",
            (Nintendo, LeftTrigger) => "ZL",
            (Nintendo, RightTrigger) => "ZR",
            (_, LeftBumper) => "L1",
            (_, RightBumper) => "R1",
            (_, LeftTrigger) => "L2",
            (_, RightTrigger) => "R2",
            (_, LeftStick) => "L3",
            (_, RightStick) => "R3",
            _ => "",
        }
    }

    fn draw(self, button: Button, canvas: &mut Canvas, left: f32) {
        use Button::*;
        let width = button.width() as f32;
        let side = SIDE as f32;
        let middle = [left + width / 2.0, side / 2.0];
        let radius = side * 0.42;
        match button {
            South | East | West | North => {
                let at = [South, East, West, North]
                    .iter()
                    .position(|face| *face == button)
                    .unwrap_or(0);
                match self {
                    Kind::Xbox => {
                        canvas.disc(middle, radius, XBOX_FACE[at]);
                        canvas.text(["A", "B", "X", "Y"][at], middle, side * 0.42, INK);
                    }
                    Kind::PlayStation => {
                        canvas.ring(middle, radius, DARK, EDGE);
                        canvas.ps_shape(at, middle, side * 0.2, PS_FACE[at]);
                    }
                    Kind::Nintendo | Kind::Deck => {
                        canvas.ring(middle, radius, DARK, EDGE);
                        // Nintendo's letters sit across from Xbox's.
                        let letters = if self == Kind::Nintendo {
                            ["B", "A", "Y", "X"]
                        } else {
                            ["A", "B", "X", "Y"]
                        };
                        canvas.text(letters[at], middle, side * 0.42, INK);
                    }
                }
            }
            LeftBumper | RightBumper | LeftTrigger | RightTrigger => {
                let half = [width / 2.0 - 3.0, side * 0.3];
                let round = if matches!(button, LeftTrigger | RightTrigger) {
                    side * 0.22
                } else {
                    side * 0.14
                };
                canvas.round_rect(middle, half, round, DARK, EDGE);
                canvas.text(self.label(button), middle, side * 0.36, INK);
            }
            LeftStick | RightStick => {
                canvas.ring(middle, radius, DARK, EDGE);
                canvas.text(self.label(button), middle, side * 0.34, INK);
            }
            Up | Down | Left | Right => canvas.dpad(button, middle, radius),
            Select | Start | Mode => {
                canvas.ring(middle, radius, DARK, EDGE);
                self.middle_button(button, canvas, middle);
            }
        }
    }

    /// The middle buttons' marks: Xbox's and the Deck's View (two windows)
    /// and Menu (three bars), PlayStation's Create (three upright bars) and
    /// Options (three bars), Nintendo's minus and plus; the guide a dot.
    fn middle_button(self, button: Button, canvas: &mut Canvas, [x, y]: [f32; 2]) {
        let s = SIDE as f32;
        let bars = |canvas: &mut Canvas, upright: bool| {
            for at in [-1.0, 0.0, 1.0] {
                let d = at * s * 0.12;
                let (a, b) = if upright {
                    ([x + d, y - s * 0.14], [x + d, y + s * 0.14])
                } else {
                    ([x - s * 0.15, y + d], [x + s * 0.15, y + d])
                };
                canvas.line(a, b, 1.6, INK);
            }
        };
        match (self, button) {
            (_, Button::Mode) => canvas.disc([x, y], s * 0.14, INK),
            (Kind::Nintendo, Button::Select) => canvas.text("-", [x, y], s * 0.4, INK),
            (Kind::Nintendo, _) => canvas.text("+", [x, y], s * 0.4, INK),
            (Kind::PlayStation, Button::Select) => bars(canvas, true),
            (Kind::PlayStation, _) | (_, Button::Start) => bars(canvas, false),
            _ => {
                let w = s * 0.17;
                canvas.rect_line([x - w * 0.6, y - w * 0.6], w, 1.6, INK);
                canvas.rect_line([x + w * 0.6, y + w * 0.6], w, 1.6, INK);
            }
        }
    }
}

/// Pixels being drawn, each a colour with its coverage, shapes laid over
/// one another with anti-aliased edges (a shape's coverage from its signed
/// distance).
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<Rgba>,
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// The distance from `p` to the segment `a`–`b`.
fn to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let along = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-6);
    let t = along.clamp(0.0, 1.0);
    length(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)
}

/// A rounded box's signed distance (`half` its half size, `round` its
/// corners' radius).
fn to_round_rect(p: [f32; 2], middle: [f32; 2], half: [f32; 2], round: f32) -> f32 {
    let qx = (p[0] - middle[0]).abs() - half[0] + round;
    let qy = (p[1] - middle[1]).abs() - half[1] + round;
    length(qx.max(0.0), qy.max(0.0)) + qx.max(qy).min(0.0) - round
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        Canvas {
            width,
            height,
            pixels: vec![[0.0; 4]; (width * height) as usize],
        }
    }

    /// `color` laid over every pixel by the coverage of the shape whose
    /// signed distance (pixels, negative inside) `distance` gives.
    fn fill(&mut self, color: Rgba, distance: impl Fn([f32; 2]) -> f32) {
        for y in 0..self.height {
            for x in 0..self.width {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let cover = (0.5 - distance(p)).clamp(0.0, 1.0) * color[3];
                if cover <= 0.0 {
                    continue;
                }
                let under = &mut self.pixels[(y * self.width + x) as usize];
                let alpha = cover + under[3] * (1.0 - cover);
                for c in 0..3 {
                    under[c] =
                        (color[c] * cover + under[c] * under[3] * (1.0 - cover)) / alpha.max(1e-6);
                }
                under[3] = alpha;
            }
        }
    }

    fn disc(&mut self, middle: [f32; 2], radius: f32, color: Rgba) {
        self.fill(color, |p| {
            length(p[0] - middle[0], p[1] - middle[1]) - radius
        });
    }

    /// A disc of `fill` with an `edge` rim.
    fn ring(&mut self, middle: [f32; 2], radius: f32, fill: Rgba, edge: Rgba) {
        self.disc(middle, radius, edge);
        self.disc(middle, radius - 2.0, fill);
    }

    fn circle_line(&mut self, middle: [f32; 2], radius: f32, width: f32, color: Rgba) {
        self.fill(color, |p| {
            (length(p[0] - middle[0], p[1] - middle[1]) - radius).abs() - width / 2.0
        });
    }

    fn round_rect(&mut self, middle: [f32; 2], half: [f32; 2], round: f32, fill: Rgba, edge: Rgba) {
        self.fill(edge, |p| to_round_rect(p, middle, half, round));
        self.fill(fill, |p| {
            to_round_rect(
                p,
                middle,
                [half[0] - 2.0, half[1] - 2.0],
                (round - 2.0).max(0.0),
            )
        });
    }

    fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: Rgba) {
        self.fill(color, |p| to_segment(p, a, b) - width / 2.0);
    }

    /// The outline of a square `side` wide around `middle`.
    fn rect_line(&mut self, middle: [f32; 2], side: f32, width: f32, color: Rgba) {
        let h = side / 2.0;
        let [x, y] = middle;
        let corners = [
            [x - h, y - h],
            [x + h, y - h],
            [x + h, y + h],
            [x - h, y + h],
        ];
        for at in 0..4 {
            self.line(corners[at], corners[(at + 1) % 4], width, color);
        }
    }

    /// PlayStation's cross, circle, square or triangle, `size` across.
    fn ps_shape(&mut self, at: usize, [x, y]: [f32; 2], size: f32, color: Rgba) {
        let width = 3.0;
        match at {
            0 => {
                self.line([x - size, y - size], [x + size, y + size], width, color);
                self.line([x + size, y - size], [x - size, y + size], width, color);
            }
            1 => self.circle_line([x, y], size, width, color),
            2 => self.rect_line([x, y], size * 1.8, width, color),
            _ => {
                let corners = [
                    [x, y - size * 1.05],
                    [x + size * 1.05, y + size * 0.8],
                    [x - size * 1.05, y + size * 0.8],
                ];
                for at in 0..3 {
                    self.line(corners[at], corners[(at + 1) % 3], width, color);
                }
            }
        }
    }

    /// The D-pad, the arm pressed lit.
    fn dpad(&mut self, button: Button, middle: [f32; 2], radius: f32) {
        let arm = radius * 0.95;
        let thick = radius * 0.36;
        let cross = |p: [f32; 2]| {
            to_round_rect(p, middle, [thick, arm], 3.0).min(to_round_rect(
                p,
                middle,
                [arm, thick],
                3.0,
            ))
        };
        self.fill(EDGE, cross);
        self.fill(DARK, |p| cross(p) + 2.0);
        let (dx, dy) = match button {
            Button::Up => (0.0, -1.0),
            Button::Down => (0.0, 1.0),
            Button::Left => (-1.0, 0.0),
            _ => (1.0, 0.0),
        };
        let lit = [middle[0] + dx * arm * 0.58, middle[1] + dy * arm * 0.58];
        let (along, across) = (arm * 0.38, thick - 3.5);
        let half = if dx == 0.0 {
            [across, along]
        } else {
            [along, across]
        };
        self.fill(INK, |p| to_round_rect(p, lit, half, 2.0));
    }

    /// `words` in strokes (`STROKES`), `height` tall, centred on `middle`.
    fn text(&mut self, words: &str, middle: [f32; 2], height: f32, color: Rgba) {
        let advance = height * 0.72;
        let start = middle[0] - advance * (words.chars().count() as f32 - 1.0) / 2.0;
        for (at, letter) in words.chars().enumerate() {
            let Some((_, strokes)) = STROKES.iter().find(|(c, _)| *c == letter) else {
                continue;
            };
            let origin = [
                start + at as f32 * advance - height * 0.32,
                middle[1] - height / 2.0,
            ];
            let place = |[u, v]: [f32; 2]| [origin[0] + u * height * 0.64, origin[1] + v * height];
            for stroke in *strokes {
                for pair in stroke.windows(2) {
                    self.line(place(pair[0]), place(pair[1]), height * 0.14, color);
                }
            }
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|pixel| pixel.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect()
    }
}

/// The letters the icons say, as strokes in a unit box (y down).
const STROKES: &[(char, &[&[[f32; 2]]])] = &[
    (
        'A',
        &[
            &[[0.1, 1.0], [0.5, 0.0], [0.9, 1.0]],
            &[[0.27, 0.6], [0.73, 0.6]],
        ],
    ),
    (
        'B',
        &[
            &[[0.2, 0.0], [0.2, 1.0]],
            &[
                [0.2, 0.0],
                [0.62, 0.0],
                [0.78, 0.12],
                [0.78, 0.38],
                [0.62, 0.5],
                [0.2, 0.5],
            ],
            &[
                [0.62, 0.5],
                [0.82, 0.62],
                [0.82, 0.88],
                [0.64, 1.0],
                [0.2, 1.0],
            ],
        ],
    ),
    (
        'X',
        &[&[[0.12, 0.0], [0.88, 1.0]], &[[0.88, 0.0], [0.12, 1.0]]],
    ),
    (
        'Y',
        &[
            &[[0.12, 0.0], [0.5, 0.5], [0.88, 0.0]],
            &[[0.5, 0.5], [0.5, 1.0]],
        ],
    ),
    ('L', &[&[[0.22, 0.0], [0.22, 1.0], [0.82, 1.0]]]),
    (
        'R',
        &[
            &[
                [0.2, 1.0],
                [0.2, 0.0],
                [0.62, 0.0],
                [0.8, 0.13],
                [0.8, 0.37],
                [0.62, 0.5],
                [0.2, 0.5],
            ],
            &[[0.5, 0.5], [0.84, 1.0]],
        ],
    ),
    (
        'S',
        &[&[
            [0.82, 0.12],
            [0.66, 0.0],
            [0.3, 0.0],
            [0.16, 0.12],
            [0.16, 0.38],
            [0.3, 0.5],
            [0.7, 0.5],
            [0.84, 0.62],
            [0.84, 0.88],
            [0.7, 1.0],
            [0.32, 1.0],
            [0.16, 0.88],
        ]],
    ),
    ('T', &[&[[0.1, 0.0], [0.9, 0.0]], &[[0.5, 0.0], [0.5, 1.0]]]),
    (
        'Z',
        &[&[[0.14, 0.0], [0.86, 0.0], [0.14, 1.0], [0.86, 1.0]]],
    ),
    (
        '1',
        &[
            &[[0.28, 0.2], [0.56, 0.0], [0.56, 1.0]],
            &[[0.28, 1.0], [0.82, 1.0]],
        ],
    ),
    (
        '2',
        &[&[
            [0.18, 0.18],
            [0.34, 0.0],
            [0.68, 0.0],
            [0.84, 0.15],
            [0.84, 0.38],
            [0.18, 1.0],
            [0.86, 1.0],
        ]],
    ),
    (
        '3',
        &[&[
            [0.18, 0.04],
            [0.82, 0.04],
            [0.5, 0.42],
            [0.68, 0.45],
            [0.84, 0.6],
            [0.84, 0.85],
            [0.66, 1.0],
            [0.32, 1.0],
            [0.16, 0.9],
        ]],
    ),
    (
        '+',
        &[&[[0.5, 0.15], [0.5, 0.85]], &[[0.12, 0.5], [0.88, 0.5]]],
    ),
    ('-', &[&[[0.12, 0.5], [0.88, 0.5]]]),
];

#[cfg(feature = "probes")]
pub(crate) fn probe() -> Result<String, String> {
    let kinds = ["xbox", "playstation", "nintendo", "deck"];
    let buttons = [
        "south",
        "east",
        "west",
        "north",
        "lefttrigger",
        "righttrigger",
        "lefttrigger2",
        "righttrigger2",
        "leftthumb",
        "rightthumb",
        "dpadup",
        "dpaddown",
        "dpadleft",
        "dpadright",
        "select",
        "start",
        "mode",
    ];
    let mut made = 0;
    for kind in kinds {
        for button in buttons {
            for chord in [button.to_string(), format!("lb+{button}")] {
                let name = format!("{PREFIX}{kind}_{chord}");
                let (width, height, rgba) =
                    pad_icon(&name).ok_or_else(|| format!("{name}: not made"))?;
                let drawn = rgba.chunks(4).filter(|pixel| pixel[3] > 0).count();
                if rgba.len() != (width * height * 4) as usize || drawn < 200 {
                    return Err(format!("{name}: {width}x{height}, {drawn} pixels drawn"));
                }
                made += 1;
            }
        }
    }
    let refused = pad_icon("kjkpad_xbox_nonsense").is_none() && pad_icon("other").is_none();
    if !refused {
        return Err("a name not made here made one".into());
    }
    Ok(format!("{made} icons made, each drawn; others refused"))
}
