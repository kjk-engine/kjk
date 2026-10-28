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

/// Where a layout of `size` sits in a window, as KOTOR draws its screens:
/// at its own size, a layout pixel a window pixel, centred on whole
/// pixels; scaled down uniformly to fit a window smaller than it. Returns
/// the scale and the layout's top left in window pixels.
pub fn fit(window: (f32, f32), size: (f32, f32)) -> (f32, [f32; 2]) {
    let (scale, [x, y]) = fill(window, size);
    if scale < 1.0 {
        return (scale, [x, y]);
    }
    let centred = |window: f32, size: f32| ((window - size) / 2.0).floor();
    (1.0, [centred(window.0, size.0), centred(window.1, size.1)])
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
    fn font(&mut self, source: Source, name: &str) -> Option<(TextureId, Arc<Font>)>;
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
