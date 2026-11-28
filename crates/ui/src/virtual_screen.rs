//! The catalogue's virtual screen (docs/UI.md, "The catalogue in the game"):
//! a screen is drawn into an image of a device's exact pixel size, and the
//! window shows that image scaled to fit, inside the device's outline.
//! Everything that decides
//! where the image stands in the window, and which point of the image a point
//! of the window is, is here and has no window or GPU in it, so the probes
//! check it.

use crate::catalogue::Size;

/// The diagonal in inches of the typical screen of each size
/// (`catalogue::SIZES`), which its outline is drawn at (`device_frame`).
pub fn inches(size: &Size) -> f32 {
    match size.name {
        "deck" => 7.0,
        "4k" => 55.0,
        "4x3" => 20.0,
        "ultrawide" => 34.0,
        _ => 24.0,
    }
}

/// What decides where the image stands in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The image's size, in the device's pixels.
    pub image: (u32, u32),
    /// The window's size in its logical pixels, and the physical pixels of
    /// the display to each of them.
    pub window: (f32, f32),
    pub window_factor: f32,
    /// How far the device's outline reaches past the image on the left,
    /// top, right and bottom, in the device's pixels
    /// (`device_frame::margin`). The image is scaled so that the outline
    /// fits the window too. It is zero when no outline is drawn.
    pub margin: [f32; 4],
}

/// Where the image stands in the window, in the window's logical pixels from
/// its top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: [f32; 2],
    pub size: [f32; 2],
    /// Logical pixels of the window to each pixel of the image.
    pub scale: f32,
}

/// Where the image stands in the window: as large as the window allows with
/// its outline around it, and in the middle. Its corner is on a whole
/// physical pixel of the display, so that a pixel of the image is not drawn
/// across two.
pub fn place(view: &View) -> Placement {
    let factor = view.window_factor.max(f32::EPSILON);
    let image = [view.image.0 as f32, view.image.1 as f32];
    let window = [view.window.0, view.window.1];
    let margin = view.margin;
    let extent = [
        image[0] + margin[0] + margin[2],
        image[1] + margin[1] + margin[3],
    ];
    let scale = (window[0] / extent[0].max(1.0)).min(window[1] / extent[1].max(1.0));
    let size = [image[0] * scale, image[1] * scale];
    let mut origin = [0.0; 2];
    for axis in 0..2 {
        origin[axis] = (window[axis] - extent[axis] * scale) / 2.0 + margin[axis] * scale;
        origin[axis] = (origin[axis] * factor).round() / factor;
    }
    Placement {
        origin,
        size,
        scale,
    }
}

impl Placement {
    /// The point of the image, in the device's pixels, that `point` of the
    /// window (its logical pixels) shows. None when the point is in the bars
    /// around the image, or past its sides.
    pub fn to_image(&self, point: [f32; 2]) -> Option<[f32; 2]> {
        let across = [
            (point[0] - self.origin[0]) / self.scale,
            (point[1] - self.origin[1]) / self.scale,
        ];
        let inside = (0..2).all(|axis| {
            point[axis] >= self.origin[axis] && point[axis] < self.origin[axis] + self.size[axis]
        });
        inside.then_some(across)
    }

    /// The point of the window where `pixel` of the image is shown.
    pub fn to_window(&self, pixel: [f32; 2]) -> [f32; 2] {
        [
            self.origin[0] + pixel[0] * self.scale,
            self.origin[1] + pixel[1] * self.scale,
        ]
    }
}
