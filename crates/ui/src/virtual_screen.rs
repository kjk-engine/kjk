//! The catalogue's virtual screen (docs/UI.md, "The catalogue in the game"):
//! a screen is drawn into an image of a device's exact pixel size, and the
//! window shows that image in one of three modes. Everything that decides
//! where the image stands in the window, and which point of the image a point
//! of the window is, is here and has no window or GPU in it, so the probes
//! check it.

use crate::catalogue::Size;

/// How the window shows the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The whole image, scaled to fit the window, with black bars.
    Fit,
    /// One pixel of the image is one physical pixel of the display.
    Pixel,
    /// The image is scaled so that a pixel looks as big as it does on the
    /// device, seen from the device's usual distance (`seen_scale`).
    Seen,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Fit, Mode::Pixel, Mode::Seen];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Fit => "fit",
            Mode::Pixel => "pixel",
            Mode::Seen => "seen",
        }
    }

    pub fn named(name: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// The mode after this one, going round.
    pub fn next(self) -> Mode {
        let at = Mode::ALL.iter().position(|mode| *mode == self).unwrap_or(0);
        Mode::ALL[(at + 1) % Mode::ALL.len()]
    }
}

/// The owner's distance from their own display, in centimetres, until a
/// setting says another.
pub const OWNER_DISTANCE_CM: f32 = 60.0;

/// The display's pixels to each inch, until a setting or the operating
/// system says another.
pub const DISPLAY_DENSITY: f32 = 110.0;

/// A device's screen and where it is usually seen from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewing {
    /// The screen's diagonal in inches.
    pub inches: f32,
    /// The distance from the eyes to the screen in centimetres.
    pub distance_cm: f32,
}

/// The typical screen and distance of each of the six sizes
/// (`catalogue::SIZES`), from the plan of the UI catalogue.
pub fn viewing(size: &Size) -> Viewing {
    let (inches, distance_cm) = match size.name {
        "deck" => (7.0, 35.0),
        "1080p" => (24.0, 60.0),
        "4k-tv" => (55.0, 250.0),
        "4k-monitor" => (27.0, 60.0),
        "4x3" => (20.0, 60.0),
        "ultrawide" => (34.0, 70.0),
        _ => (24.0, 60.0),
    };
    Viewing {
        inches,
        distance_cm,
    }
}

/// How many pixels of a screen `pixels` large, `inches` across its diagonal,
/// fit into one inch.
pub fn pixels_per_inch(pixels: (u32, u32), inches: f32) -> f32 {
    let diagonal = (pixels.0 as f32).hypot(pixels.1 as f32);
    diagonal / inches.max(f32::EPSILON)
}

/// How many pixels of the owner's display one pixel of the device covers in
/// the owner's eye, when the device is seen from its usual distance and the
/// display from the owner's.
///
/// A pixel looks as big as its width divided by its distance. The device's
/// pixel is one inch over its density wide, and so is the display's.
/// The scale is the device pixel's apparent size over the display pixel's.
pub fn seen_scale(
    device: (Size, Viewing),
    (display_density, owner_distance_cm): (f32, f32),
) -> f32 {
    let (size, viewing) = device;
    let device_density = pixels_per_inch(size.pixels, viewing.inches);
    let device_apparent = 1.0 / (device_density * viewing.distance_cm.max(f32::EPSILON));
    let display_apparent =
        1.0 / (display_density.max(f32::EPSILON) * owner_distance_cm.max(f32::EPSILON));
    device_apparent / display_apparent
}

/// What decides where the image stands in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub mode: Mode,
    /// The image's size, in the device's pixels.
    pub image: (u32, u32),
    /// The window's size in its logical pixels, and the physical pixels of
    /// the display to each of them.
    pub window: (f32, f32),
    pub window_factor: f32,
    /// How many display pixels a device pixel covers in `Mode::Seen`.
    pub seen: f32,
    /// How far the window's top left corner is into the shown image, in the
    /// window's logical pixels, when the image is larger than the window.
    pub pan: [f32; 2],
}

/// Where the image stands in the window, in the window's logical pixels from
/// its top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: [f32; 2],
    pub size: [f32; 2],
    /// Logical pixels of the window to each pixel of the image.
    pub scale: f32,
    /// The pan that was kept, within what the image can move.
    pub pan: [f32; 2],
}

/// Where the image stands in the window. An image larger than the window
/// along an axis moves along it with `pan`, kept within the image. Along the
/// other axes it stays in the middle. Its corner is on a whole physical pixel
/// of the display, so that a pixel of the image is not drawn across two.
pub fn place(view: &View) -> Placement {
    let factor = view.window_factor.max(f32::EPSILON);
    let image = [view.image.0 as f32, view.image.1 as f32];
    let window = [view.window.0, view.window.1];
    let scale = match view.mode {
        Mode::Fit => (window[0] / image[0].max(1.0)).min(window[1] / image[1].max(1.0)),
        Mode::Pixel => 1.0 / factor,
        Mode::Seen => view.seen / factor,
    };
    let size = [image[0] * scale, image[1] * scale];
    let mut origin = [0.0; 2];
    let mut pan = [0.0; 2];
    for axis in 0..2 {
        let spare = size[axis] - window[axis];
        if spare > 0.0 {
            pan[axis] = view.pan[axis].clamp(0.0, spare);
            origin[axis] = -pan[axis];
        } else {
            origin[axis] = (window[axis] - size[axis]) / 2.0;
        }
        origin[axis] = (origin[axis] * factor).round() / factor;
    }
    Placement {
        origin,
        size,
        scale,
        pan,
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

    /// How far the image can be panned along each axis, in the window's
    /// logical pixels. Zero where the image fits the window.
    pub fn spare(&self, window: (f32, f32)) -> [f32; 2] {
        [
            (self.size[0] - window.0).max(0.0),
            (self.size[1] - window.1).max(0.0),
        ]
    }
}
