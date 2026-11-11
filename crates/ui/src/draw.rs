//! What a screen draws: textured quads in the layout's own pixels (top
//! left origin), drawn in list order over each other. The same list goes
//! to the GPU in the game and to `raster` for headless pictures.

use std::sync::Arc;

use asset_odyssey::font::Font;

use crate::skin::Source;

/// A texture the drawer knows by number; 0 is plain white.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TextureId(pub u32);

impl TextureId {
    pub const WHITE: TextureId = TextureId(0);
    /// The first of the 3D views' ids (`Textures::scene`).
    pub const SCENES: u32 = 1 << 31;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Left, top, width, height in layout pixels.
    pub rect: [f32; 4],
    /// u0, v0, u1, v1 on the upright texture; u0 > u1 mirrors.
    pub uv: [f32; 4],
    /// Quarter turns of the texture, clockwise, before `uv` is read.
    pub turns: u8,
    pub texture: TextureId,
    /// Multiplies the texture; alpha blends.
    pub color: [f32; 4],
    /// Adds instead of blending (the texture's TXI says `additive`).
    pub additive: bool,
    /// Left, top, width, height outside which nothing of the quad shows,
    /// before it turns by `spin`.
    pub clip: Option<[f32; 4]>,
    /// How far the quad, clipped, turns about its rect's centre: radians,
    /// clockwise on the screen.
    pub spin: f32,
}

impl Quad {
    pub fn solid(rect: [f32; 4], color: [f32; 4]) -> Quad {
        Quad {
            rect,
            uv: [0.0, 0.0, 1.0, 1.0],
            turns: 0,
            texture: TextureId::WHITE,
            color,
            additive: false,
            clip: None,
            spin: 0.0,
        }
    }

    pub fn image(rect: [f32; 4], texture: TextureId, color: [f32; 4]) -> Quad {
        Quad {
            texture,
            ..Quad::solid(rect, color)
        }
    }
}

/// `point` turned `angle` radians clockwise on the screen (y down) about
/// `centre`; unturned exactly at 0.
pub fn turn(point: [f32; 2], centre: [f32; 2], angle: f32) -> [f32; 2] {
    if angle == 0.0 {
        return point;
    }
    let (sin, cos) = angle.sin_cos();
    let [x, y] = [point[0] - centre[0], point[1] - centre[1]];
    [centre[0] + x * cos - y * sin, centre[1] + x * sin + y * cos]
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawList {
    /// The layout's own size (800 × 600 for KOTOR 2's screens).
    pub size: (f32, f32),
    /// Stretched to fill the window (`fill`) rather than drawn at its own
    /// size (`fit`): a frame already of the window's shape, sized to its
    /// height (the conversation's, a bark's).
    pub fill: bool,
    pub quads: Vec<Quad>,
    /// The textures whose quads reach the window's sides past the layout's
    /// (`ControlState::wide`): their height the layout's, their width the
    /// window's.
    pub wide: Vec<TextureId>,
    /// The 3D views whose image keeps a size other than their quad's
    /// (`ControlState::scene_size`), and that size in layout pixels.
    pub sized: Vec<(TextureId, [f32; 2])>,
}

impl DrawList {
    /// Where it sits in a window of `window` pixels (`fit` or `fill`).
    pub fn placed(&self, window: (f32, f32)) -> (f32, [f32; 2]) {
        if self.fill {
            fill(window, self.size)
        } else {
            fit(window, self.size)
        }
    }
}

/// The window height up to which a layout pixel is a window pixel
/// (`scale`).
pub const UI_HEIGHT: f32 = 720.0;

/// How many window pixels a layout pixel takes in a window of `window`
/// pixels: one, as KOTOR draws its screens, up to `UI_HEIGHT`; above it in
/// proportion to the height, so the screens look the same size at any
/// resolution and any desktop scaling.
pub fn scale(window: (f32, f32)) -> f32 {
    (window.1 / UI_HEIGHT).max(1.0)
}

/// How many times its font's size Aspyr's KOTOR 2 draws text in a window
/// of `window` physical pixels. The text is drawn at its own size up to
/// 2045 pixels wide. From there on it grows by half its size for each
/// further 1024 pixels of width: 1.5 times at 2560 pixels wide, 2 times at
/// 3840.
///
/// Only this module and its probes read it. Everything else reads the text
/// size from a `Sizing`, which multiplies it by the player's text scale
/// (`TextScale`).
pub(crate) fn text_size(window: (f32, f32)) -> f32 {
    if window.0 > 1022.0 {
        1.0 + 0.5 * ((window.0 - 1022.0) / 1024.0).floor()
    } else {
        1.0
    }
}

/// KOTOR 2's "Use Small Fonts" option, which only its ini file sets (in
/// "Game Options"), as the settings file alone sets it here. It picks
/// between each font's two sheets: the "a" sheet, made for small windows,
/// and the "b" sheet, made for large ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmallFonts {
    /// The ini's 0: the "b" sheets in a window wider than 1279 pixels,
    /// else the "a" sheets.
    #[default]
    Auto,
    /// The ini's 1: the "a" sheets in any window.
    Small,
    /// The ini's 2: the "b" sheets in any window.
    Large,
}

impl SmallFonts {
    /// The setting that holds it: `auto`, `small` or `large`.
    pub const SETTING: &'static str = "display.small_fonts";

    /// The option a setting's value names; `Auto` for any other value.
    pub fn named(value: &str) -> SmallFonts {
        match value {
            "small" => SmallFonts::Small,
            "large" => SmallFonts::Large,
            _ => SmallFonts::Auto,
        }
    }

    /// The letter KOTOR 2 puts after a layout's font name in a window
    /// `width` physical pixels wide.
    pub fn suffix(self, width: f32) -> char {
        match self {
            SmallFonts::Small => 'a',
            SmallFonts::Large => 'b',
            SmallFonts::Auto if width > 1279.0 => 'b',
            SmallFonts::Auto => 'a',
        }
    }
}

/// kjk's Interface option "Text scale". KOTOR 2 has no such option: it draws
/// its text at one size for a window (`text_size`). The option multiplies
/// that size everywhere kjk draws interface text. A `Sizing` carries the
/// multiplier, so nothing draws or measures text without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextScale;

impl TextScale {
    /// The setting that holds it: a percentage from `PERCENTS`.
    pub const SETTING: &'static str = "display.text_scale";

    /// The setting's values. They are the percentages 80 to 150 in steps
    /// of 10.
    pub const PERCENTS: [&'static str; 8] = ["80", "90", "100", "110", "120", "130", "140", "150"];

    /// The value of a new settings file. It is KOTOR 2's own size.
    pub const DEFAULT: &'static str = "100";

    /// How many times KOTOR 2's size the text is drawn for a setting's
    /// value. Any value that is not one of `PERCENTS` gives 1, which is
    /// KOTOR 2's own size.
    pub fn named(value: &str) -> f32 {
        Self::PERCENTS
            .contains(&value)
            .then(|| value.parse::<f32>().ok())
            .flatten()
            .map_or(1.0, |percent| percent / 100.0)
    }

    /// The text scale `settings` hold (`named`). It is 1 while unset.
    pub fn of(settings: &std::collections::BTreeMap<String, String>) -> f32 {
        settings
            .get(Self::SETTING)
            .map_or(1.0, |value| Self::named(value))
    }
}

/// How large text and borders are drawn, and which font sheets are read.
/// The textures hold one such value (`Textures::sizing`). The fonts, the
/// painter and everything that measures words or borders read that one
/// value, so that what is measured is what is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizing {
    /// The size fonts are drawn at, in layout pixels to a font pixel. The
    /// player's text scale is already multiplied into it.
    pub text: f32,
    /// One of the display's physical pixels in layout pixels. KOTOR 2 draws
    /// a border's corners and edges at its DIMENSION in physical pixels,
    /// however far the control itself is stretched. A bigger window makes a
    /// button bigger but its corners no rounder. Text is placed on whole
    /// physical pixels.
    pub pixel: f32,
    /// The letter KOTOR 2 puts after fonts' names (`SmallFonts::suffix`),
    /// where a sheet of that name exists. `None` reads every font by the
    /// name as written.
    pub suffix: Option<char>,
    /// The player's text scale (`TextScale`), which `text` already holds.
    /// It is kept here so that every sizing made from this one (`at`,
    /// `outside`) and every text size read from it (`text_size`) has the
    /// same scale.
    pub scale: f32,
}

impl Default for Sizing {
    /// A layout pixel is a font pixel and a physical pixel, fonts are read
    /// by their names as written, and the text scale is 1.
    fn default() -> Sizing {
        Sizing {
            text: 1.0,
            pixel: 1.0,
            suffix: None,
            scale: 1.0,
        }
    }
}

impl Sizing {
    /// KOTOR 2's sizes of text and borders for a layout drawn `scale`
    /// window pixels to each of its own pixels, in a window of `window`
    /// logical pixels whose display has `factor` physical pixels to each
    /// (`UiInput::scale_factor`), with "Use Small Fonts" set to `small` and
    /// kjk's text scale set to `text_scale` times (`TextScale`).
    ///
    /// Aspyr's KOTOR 2 runs at the display's physical resolution, so each
    /// size rule it has counts physical pixels. A font pixel is `text_size`
    /// physical pixels wide, `text_size` reading the window's physical
    /// width. A border's corners and edges are its DIMENSION in physical
    /// pixels. The font sheets are picked by the window's physical width.
    /// On a Retina Mac, with 2 physical pixels to each logical one, text
    /// and borders are half as many logical pixels as on a display with 1.
    pub fn physical(
        window: (f32, f32),
        factor: f32,
        scale: f32,
        small: SmallFonts,
        text_scale: f32,
    ) -> Sizing {
        Sizing {
            suffix: Some(small.suffix(window.0 * factor)),
            scale: text_scale,
            ..Sizing::default()
        }
        .at(window, factor, scale)
    }

    /// How many times its font's size text is drawn in a window of
    /// `physical` physical pixels. It is Aspyr's size for the window
    /// (`text_size`) times the player's text scale. The boxes that KOTOR 2
    /// sizes from its text size read this one.
    pub fn text_size(self, physical: (f32, f32)) -> f32 {
        text_size(physical) * self.scale
    }

    /// This sizing with the font letter and the text scale of `stepped`.
    /// The stack leaves it for what is drawn outside the stack after its
    /// step. Those layers then read the same font sheets and draw their
    /// text at the same scale as the stack's screens.
    pub fn outside(self, stepped: Sizing) -> Sizing {
        Sizing {
            text: self.text / self.scale * stepped.scale,
            suffix: stepped.suffix,
            scale: stepped.scale,
            ..self
        }
    }

    /// The same font sheets with the text and the physical pixel of a
    /// layout drawn `scale` window pixels to each of its own pixels
    /// (`physical`). The layers drawn outside the stack use it, each at its
    /// own scale, with the font letter and the text scale the stack set.
    pub fn at(self, window: (f32, f32), factor: f32, scale: f32) -> Sizing {
        let per_layout_pixel = factor.max(f32::EPSILON) * scale.max(f32::EPSILON);
        let wide = (window.0 * factor, window.1 * factor);
        Sizing {
            text: self.text_size(wide) / per_layout_pixel,
            pixel: 1.0 / per_layout_pixel,
            ..self
        }
    }
}

/// Runs `run` with `sizing` as the textures' sizing, and puts the sizing
/// they had before back afterwards.
pub fn sized<T: Textures + ?Sized, R>(
    textures: &mut T,
    sizing: Sizing,
    run: impl FnOnce(&mut T) -> R,
) -> R {
    let before = textures.sizing();
    textures.set_sizing(sizing);
    let result = run(textures);
    textures.set_sizing(before);
    result
}

/// How wide the stack draws a full-screen menu, kjk's Interface option.
/// KOTOR 2's options have no such setting: it always stretches its menus
/// to the window (`Fill`), which an unset setting gives. kjk's settings
/// file starts at `UpTo16x9` (`settings::DEFAULTS` in the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuWidth {
    /// `fill`: KOTOR 2's stretch, to the window's edges.
    #[default]
    Fill,
    /// `16x9`: the same stretch in a window up to 16:9. A wider window
    /// gets the menu as wide as a 16:9 one of its height, centred, with
    /// black bars at its sides.
    UpTo16x9,
    /// `4x3`: the layout's own shape, scaled evenly and centred (`fit`),
    /// with black bars round it.
    Kept,
}

impl MenuWidth {
    /// The setting that holds it: `fill`, `16x9` or `4x3`.
    pub const SETTING: &'static str = "display.menu_width";

    /// The option a setting's value names; `Fill` for any other value.
    pub fn named(value: &str) -> MenuWidth {
        match value {
            "16x9" => MenuWidth::UpTo16x9,
            "4x3" => MenuWidth::Kept,
            _ => MenuWidth::Fill,
        }
    }
}

/// The setting of kjk's Interface option "Maintain image aspect ratio":
/// `true` keeps the pictures of the game's own menus in shape when they are
/// stretched, `false` stretches them as KOTOR 2 does (`screen::stretch`).
pub const IMAGE_ASPECT: &str = "display.image_aspect";

/// The setting of kjk's Interface option "Notifications in the corner":
/// `true` slides the status summaries in at the window's right side under
/// the minimap, as kjk used to show them, and shows a box of its own for a
/// datapad the party picks up. `false` centres them as KOTOR 2 does.
pub const CORNER_NOTICES: &str = "display.corner_notices";

/// The setting of kjk's Interface option "Target block at the top centre":
/// `true` pins the HUD's target block to the top of the window, as kjk used
/// to place it. `false` floats it over the target, as KOTOR 2 does.
pub const TARGET_TOP: &str = "display.target_top";

/// Whether kjk's own Interface option `key` is on in `settings`. Each one is
/// off unless its setting says `true`, so that an unset setting behaves as
/// KOTOR 2 does. kjk's settings file starts with all of them on
/// (`settings::DEFAULTS` in the game), as they suit its real-time play.
pub fn kjk_option(settings: &std::collections::BTreeMap<String, String>, key: &str) -> bool {
    settings.get(key).map(String::as_str) == Some("true")
}

/// Whether "Maintain image aspect ratio" is on in `settings`
/// (`kjk_option`). KOTOR 2 stretches its pictures, so it is off unless the
/// setting says `true`.
pub fn image_aspect(settings: &std::collections::BTreeMap<String, String>) -> bool {
    kjk_option(settings, IMAGE_ASPECT)
}

/// Where a layout of `size` sits in a window: enlarged by the window's
/// `scale`, or less so it still fits, centred on whole pixels. Returns the
/// scale and the layout's top left in window pixels.
pub fn fit(window: (f32, f32), size: (f32, f32)) -> (f32, [f32; 2]) {
    let (most, _) = fill(window, size);
    let scale = scale(window).min(most);
    let centred = |window: f32, size: f32| ((window - size * scale) / 2.0).floor();
    (
        scale,
        [centred(window.0, size.0), centred(window.1, size.1)],
    )
}

/// Where a layout of `size` sits in a window stretched to fill it:
/// scaled uniformly to fit, larger or smaller, and centred.
pub fn fill(window: (f32, f32), size: (f32, f32)) -> (f32, [f32; 2]) {
    let scale = (window.0 / size.0.max(1.0)).min(window.1 / size.1.max(1.0));
    let offset = [
        (window.0 - size.0 * scale) / 2.0,
        (window.1 - size.1 * scale) / 2.0,
    ];
    (scale, offset)
}

/// A window point in the pixels of a layout of `size` placed at
/// `placed` (`fit`, `fill`); `None` outside the layout.
pub fn window_to_layout(
    placed: (f32, [f32; 2]),
    size: (f32, f32),
    point: [f32; 2],
) -> Option<[f32; 2]> {
    let (scale, [x, y]) = placed;
    let at = [(point[0] - x) / scale, (point[1] - y) / scale];
    let inside = (0.0..=size.0).contains(&at[0]) && (0.0..=size.1).contains(&at[1]);
    inside.then_some(at)
}

/// What painting needs of the textures: numbers for names, sizes, fonts,
/// each read from the install `source` says.
pub trait Textures {
    /// The texture of a resource name, loading it on first use; `None`
    /// when it does not resolve.
    fn texture(&mut self, source: Source, name: &str) -> Option<TextureId>;
    fn size(&self, id: TextureId) -> (u32, u32);
    /// Drawn adding (`blending additive` in its TXI).
    fn additive(&self, id: TextureId) -> bool;
    /// A font at the sizing's text size and from the sheet its letter
    /// names (`sizing`).
    fn font(&mut self, source: Source, name: &str) -> Option<(TextureId, Arc<Font>)>;
    /// How large text and borders are drawn now, and which font sheets are
    /// read. It is `Sizing::default()` where it is not kept.
    fn sizing(&self) -> Sizing {
        Sizing::default()
    }
    /// Text and borders are drawn at `sizing` from now on (`sized` sets it
    /// for one piece of work and puts the earlier one back).
    fn set_sizing(&mut self, _sizing: Sizing) {}
    /// A talk-table entry's text.
    fn text(&self, source: Source, strref: u32) -> Option<Vec<u8>>;
    /// Whether what `source` reads is recoloured to the campaign's colours
    /// (its textures already are; a layout's colours are by the painter).
    fn recoloured(&self, _source: Source) -> bool {
        false
    }
    /// The texture a 3D view draws into (the game renders it); `None`
    /// where nothing renders them.
    fn scene(&mut self, _view: &crate::vm::SceneView) -> Option<TextureId> {
        None
    }
}
