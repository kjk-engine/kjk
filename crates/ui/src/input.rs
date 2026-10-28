//! What reaches the screens in a frame, whatever the device: keys,
//! gamepad buttons and the pointer, already turned into the same few
//! moves.

/// One move of the focus or the open screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    /// Enter, gamepad A.
    Activate,
    /// Escape, gamepad B or Start.
    Back,
    /// The menu bar's next and previous tab (gamepad RB and LB).
    NextTab,
    PrevTab,
    ScrollUp,
    ScrollDown,
    Backspace,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiInput {
    pub keys: Vec<Key>,
    /// Characters typed this frame.
    pub text: String,
    /// The keys pressed this frame by name (`KeyW`), for binding them.
    pub raw: Vec<String>,
    /// Where the pointer is, in window pixels.
    pub pointer: Option<[f32; 2]>,
    /// The pointer moved this frame.
    pub moved: bool,
    /// The main button was pressed this frame.
    pub click: bool,
    /// Wheel notches, up positive.
    pub wheel: f32,
    /// The window's size in pixels.
    pub window: (f32, f32),
    /// Seconds since the last frame.
    pub dt: f32,
}
