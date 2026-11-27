//! The backdrop behind the screen, which a KOTOR panel's see-through parts
//! show.

use ui::draw::{DrawList, Quad};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backdrop {
    /// Black, as the catalogue always drew it.
    #[default]
    Plain,
    /// Diagonal stripes of two greys.
    Stripes,
}

impl Backdrop {
    /// Every backdrop in the order the key cycles them. A frame of play
    /// would join here, once a still of play is cheap to get.
    pub const ALL: [Backdrop; 2] = [Backdrop::Plain, Backdrop::Stripes];

    pub fn name(self) -> &'static str {
        match self {
            Backdrop::Plain => "plain",
            Backdrop::Stripes => "stripes",
        }
    }

    pub fn named(name: &str) -> Option<Backdrop> {
        Backdrop::ALL
            .into_iter()
            .find(|backdrop| backdrop.name() == name)
    }

    pub fn next(self) -> Backdrop {
        let at = Backdrop::ALL
            .iter()
            .position(|each| *each == self)
            .unwrap_or(0);
        Backdrop::ALL[(at + 1) % Backdrop::ALL.len()]
    }

    /// What is drawn under the screens in a window of `window` logical
    /// pixels, if anything.
    pub fn list(self, window: (f32, f32)) -> Option<DrawList> {
        match self {
            Backdrop::Plain => None,
            Backdrop::Stripes => Some(stripes(window)),
        }
    }
}

const DARK: [f32; 4] = [0.14, 0.14, 0.16, 1.0];
const LIGHT: [f32; 4] = [0.46, 0.46, 0.5, 1.0];

/// Stripes that run down to the right at 45 degrees, their width half their
/// spacing, 1/16 of the window's height apart.
fn stripes(window: (f32, f32)) -> DrawList {
    let spacing = (window.1 / 16.0).max(16.0);
    let reach = window.0 + window.1;
    let middle = [window.0 / 2.0, window.1 / 2.0];
    let across = std::f32::consts::FRAC_1_SQRT_2;
    let mut list = DrawList {
        size: window,
        ..DrawList::default()
    };
    list.quads
        .push(Quad::solid([0.0, 0.0, window.0, window.1], DARK));
    let mut offset = -reach / 2.0;
    while offset < reach / 2.0 {
        let centre = [middle[0] + offset * across, middle[1] - offset * across];
        let rect = [
            centre[0] - reach,
            centre[1] - spacing / 4.0,
            reach * 2.0,
            spacing / 2.0,
        ];
        list.quads.push(Quad {
            spin: std::f32::consts::FRAC_PI_4,
            ..Quad::solid(rect, LIGHT)
        });
        offset += spacing;
    }
    list
}
